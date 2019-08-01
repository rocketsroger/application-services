/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::collections::HashSet;

use sql_support::SqlInterruptScope;
use sync15::{
    CollState, CollectionKeys, CollectionRequest, CollectionUpdate, GlobalState, IncomingChangeset,
    KeyBundle, OutgoingChangeset, Payload, Sync15StorageClient,
};

use super::{
    record::{Client, ClientCommand},
    ser::shrink_to_fit,
    Settings,
};
use crate::error::{ErrorKind, Result};
use crate::manager::SyncManager;

pub struct Engine<'a> {
    manager: &'a SyncManager,
    interruptee: &'a SqlInterruptScope,
    client: &'a Sync15StorageClient,
    global_state: &'a GlobalState,
    root_sync_key: &'a KeyBundle,
    fully_atomic: bool,
    settings: Settings,
}

impl<'a> Engine<'a> {
    /// Syncs the clients collection. This works a little differently than
    /// other collections:
    ///
    ///   1. It can't be disabled or declined.
    ///   2. The sync ID and last sync time aren't meaningful, since we always
    ///      fetch all client records on every sync.
    ///   3. It syncs twice: once at the start of the sync, to apply commands
    ///      from other devices, and once at the end, to ack our commands and
    ///      send commands to other devices.
    ///   4. It doesn't persist state directly, but relies on Sync Manager
    ///      consumers to persist `ClientSettings`, and on syncable Rust
    ///      ddd
    ///   5. Failing to sync the clients collection is fatal, and aborts the
    ///      sync.
    ///
    /// For these reasons, we implement a specialized `sync` method instead of
    /// implementing `sync15::Store`, even though our methods have similar
    /// signatures.
    pub fn sync(&self) -> Result<()> {
        log::info!("Syncing collection clients");

        let coll_keys = CollectionKeys::from_encrypted_bso(
            self.global_state.keys.clone(),
            &self.root_sync_key,
        )?;
        let mut coll_state = CollState {
            config: self.global_state.config.clone(),
            last_modified: self
                .global_state
                .collections
                .get("clients")
                .cloned()
                .unwrap_or_default(),
            key: coll_keys.key_for_collection("clients").clone(),
        };

        let inbound = self.fetch_incoming(&mut coll_state)?;

        let outgoing = self.apply_incoming(inbound)?;
        coll_state.last_modified = outgoing.timestamp;

        self.interruptee.err_if_interrupted()?;
        let upload_info = CollectionUpdate::new_from_changeset(
            &self.client,
            &coll_state,
            outgoing,
            self.fully_atomic,
        )?
        .upload()?;

        log::info!(
            "Upload success ({} records success, {} records failed)",
            upload_info.successful_ids.len(),
            upload_info.failed_ids.len()
        );

        log::info!("Finished syncing clients");
        Ok(())
    }

    fn current_client_record(&self) -> Client {
        Client {
            id: self.settings.client_id.clone(),
            name: self.settings.name.clone(),
            typ: Some(self.settings.client_type.as_str().into()),
            commands: Vec::new(),
            fxa_device_id: Some(self.settings.fxa_device_id.clone()),
            version: None,
            protocols: vec!["1.5".into()],
            form_factor: None,
            os: None,
            app_package: None,
            application: None,
            device: None,
        }
    }

    fn fetch_incoming(&self, coll_state: &mut CollState) -> Result<IncomingChangeset> {
        let coll_request = CollectionRequest::new("clients").full();

        self.interruptee.err_if_interrupted()?;
        let inbound =
            IncomingChangeset::fetch(&self.client, coll_state, "clients".into(), &coll_request)?;

        Ok(inbound)
    }

    fn max_record_payload_size(&self) -> usize {
        let payload_max = self.global_state.config.max_record_payload_bytes;
        if payload_max <= self.global_state.config.max_post_bytes {
            self.global_state
                .config
                .max_post_bytes
                .checked_sub(4096)
                .unwrap_or(0)
        } else {
            payload_max
        }
    }

    /// Collections stored in memcached ("tabs", "clients" or "meta") have a
    /// different max size than ones stored in the normal storage server db.
    /// In practice, the real limit here is 1M (bug 1300451 comment 40), but
    /// there's overhead involved that is hard to calculate on the client, so we
    /// use 512k to be safe (at the recommendation of the server team). Note
    /// that if the server reports a lower limit (via info/configuration), we
    /// respect that limit instead. See also bug 1403052.
    fn memcache_max_record_payload_size(&self) -> usize {
        self.max_record_payload_size().min(512 * 1024)
    }

    fn apply_incoming(&self, inbound: IncomingChangeset) -> Result<OutgoingChangeset> {
        let mut outgoing = OutgoingChangeset::new("clients".into(), inbound.timestamp);
        outgoing.timestamp = inbound.timestamp;

        self.interruptee.err_if_interrupted()?;
        let outgoing_commands = self.manager.fetch_outgoing_commands()?;

        for (payload, _) in inbound.changes {
            self.interruptee.err_if_interrupted()?;

            // Unpack the client record. We should never have tombstones in the
            // clients collection, so we don't check for `is_tombstone`.
            // TODO(lina): The Desktop engine automatically deletes these.
            let mut client: Client = payload.into_record()?;

            if client.id == self.settings.client_id {
                let mut current_client_record = self.current_client_record();
                for c in client.commands {
                    // If we see our own client record, apply any incoming
                    // commands, remove them from the list, and reupload the
                    // record. Any commands that failed to apply, or that we
                    // don't understand, go back in the list.
                    let result = match c.as_command() {
                        Some(command) => self.manager.apply_incoming_command(command),
                        None => Err(ErrorKind::UnsupportedCommand(c.name.clone()).into()),
                    };
                    if let Err(e) = result {
                        // Put the command back into the record. We'll try
                        // again on the next sync, or when this client
                        // upgrades.
                        // TODO(lina): Failing to apply the command should
                        // be fatal (for example, we shouldn't sync after
                        // failing to wipe!), but "I don't understand this
                        // command" should put it back and continue.
                        log::warn!("Failed to apply incoming command: {:?}", e);
                        current_client_record.commands.push(c);
                    }
                }

                // The clients collection has a hard limit on the payload size,
                // after which the server starts rejecting our records. Large
                // command lists can cause us to exceed this, so we truncate
                // the list.
                shrink_to_fit(
                    &mut current_client_record.commands,
                    self.memcache_max_record_payload_size(),
                )?;

                // We always upload our own client record on each sync, even if it
                // doesn't change, to keep it fresh.
                outgoing
                    .changes
                    .push(Payload::from_record(current_client_record)?);
            } else {
                let commands = client
                    .commands
                    .iter()
                    .filter_map(ClientCommand::as_command)
                    .collect::<HashSet<_>>();
                let new_commands = outgoing_commands.difference(&commands);
                client
                    .commands
                    .extend(new_commands.into_iter().map(|&command| command.into()));
                shrink_to_fit(
                    &mut client.commands,
                    self.memcache_max_record_payload_size(),
                )?;
                outgoing.changes.push(Payload::from_record(client)?);
            }
        }

        Ok(outgoing)
    }
}

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use sync_guid::Guid as SyncGuid;

mod engine;
mod record;
mod ser;

pub use engine::Engine;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Settings {
    /// The ID for this client. This should be stable across syncs, and is
    /// used as this client's record ID in the clients collection.
    pub client_id: SyncGuid,
    /// The name of this client. This should match the client's name in the
    /// FxA device manager.
    pub name: String,
    /// The type of this client: mobile, tablet, desktop, or other.
    pub client_type: Type,
    /// The device ID of this client, linking it to the FxA device manager.
    pub fxa_device_id: String,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Type {
    Desktop,
    Mobile,
    Tablet,
}

impl Type {
    pub fn as_str(self) -> &'static str {
        match self {
            Type::Desktop => "desktop",
            Type::Mobile => "mobile",
            Type::Tablet => "tablet",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Command {
    WipeLogins,
    WipeHistory,
    WipeBookmarks,
    WipeAll,
    ResetLogins,
    ResetHistory,
    ResetBookmarks,
    ResetAll,
}

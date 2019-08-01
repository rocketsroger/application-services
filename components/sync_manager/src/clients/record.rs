/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use serde_derive::*;
use sync_guid::Guid as SyncGuid;

use super::Command;

/// A client record.
#[derive(Clone, Debug, Eq, Deserialize, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Client {
    #[serde(rename = "id")]
    pub id: SyncGuid,

    pub name: String,

    #[serde(default, rename = "type")]
    pub typ: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<ClientCommand>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fxa_device_id: Option<String>,

    /// `version`, `protocols`, `formfactor`, `os`, `appPackage`, `application`,
    /// and `device` are unused and optional in all implementations (Desktop,
    /// iOS, and Fennec), but we round-trip them.

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub protocols: Vec<String>,

    #[serde(default, rename = "formfactor" skip_serializing_if = "Option::is_none")]
    pub form_factor: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_package: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application: Option<String>,

    /// The model of the device, like "iPhone" or "iPod touch" on iOS. Note
    /// that this is _not_ the client ID (`id`) or the FxA device ID
    /// (`fxa_device_id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
}

#[derive(Clone, Debug, Eq, Deserialize, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCommand {
    /// The command name. This is a string, not an enum, because we want to
    /// round-trip commands that we don't support yet.
    #[serde(rename = "command")]
    pub name: String,

    /// Extra, command-specific arguments. Note that we must send an empty
    /// array if the command expects no arguments.
    #[serde(default)]
    pub args: Vec<String>,

    /// Some commands, like repair, send a "flow ID" that other cliennts can
    /// record in their telemetry. We don't currently send commands with
    /// flow IDs, but we round-trip them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_id: Option<String>,
}

impl ClientCommand {
    pub fn as_command(&self) -> Option<Command> {
        match self.name.as_str() {
            "wipeEngine" => match self.args.get(0).map(|s| s.as_str()) {
                Some("logins") => Some(Command::WipeLogins),
                Some("history") => Some(Command::WipeHistory),
                Some("bookmarks") => Some(Command::WipeBookmarks),
                _ => None,
            },
            "wipeAll" => Some(Command::WipeAll),
            "resetEngine" => match self.args.get(0).map(|s| s.as_str()) {
                Some("logins") => Some(Command::ResetLogins),
                Some("history") => Some(Command::ResetHistory),
                Some("bookmarks") => Some(Command::ResetBookmarks),
                _ => None,
            },
            "resetAll" => Some(Command::ResetAll),
            _ => None,
        }
    }

    #[inline]
    pub fn from_command_with_flow_id(command: Command, flow_id: String) -> ClientCommand {
        ClientCommand::from_command(command, Some(flow_id))
    }

    fn from_command(command: Command, flow_id: Option<String>) -> ClientCommand {
        let (name, args): (&str, &[&str]) = match command {
            Command::WipeLogins => ("wipeEngine", &["passwords"]),
            Command::WipeHistory => ("wipeEngine", &["history"]),
            Command::WipeBookmarks => ("wipeEngine", &["bookmarks"]),
            Command::WipeAll => ("wipeAll", &[]),
            Command::ResetLogins => ("resetEngine", &["passwords"]),
            Command::ResetHistory => ("resetEngine", &["history"]),
            Command::ResetBookmarks => ("resetEngine", &["bookmarks"]),
            Command::ResetAll => ("resetAll", &[]),
        };
        ClientCommand {
            name: name.into(),
            args: args.iter().map(|&n| n.into()).collect(),
            flow_id,
        }
    }
}

impl From<Command> for ClientCommand {
    fn from(command: Command) -> ClientCommand {
        ClientCommand::from_command(command, None)
    }
}

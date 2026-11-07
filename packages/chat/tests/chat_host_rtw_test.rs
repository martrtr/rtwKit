//! End-to-end check of the packaged Chat WASM component through a real Host.
//!
//! Build the Chat RTW first and run this ignored test. World remains
//! authoritative: a response is successful only after the SQLite COMMIT.

use anyhow::{Context, Result};
use rintawa_artifacts::{RtwLimits, pack_directory};
use rintawa_chat::{
    CONVERSATION_PROPERTY, CREATE_CONVERSATION_COMMAND, MESSAGE_PROPERTY, SEND_MESSAGE_COMMAND,
};
use rintawa_host::{HostHome, HostRuntime};
use rintawa_sdk::world::{ItemId, PrincipalId, SchemaKey};
use rintawa_storage::SqliteWorldStorage;
use rintawa_world::WorldCommand;
use serde_json::json;

fn key(name: &str) -> Result<SchemaKey> {
    Ok(name.parse()?)
}

#[test]
#[ignore = "build Chat RTW first with bash packages/chat/build.sh"]
fn test_should_commit_chat_through_real_wasm_component_and_restart_host() -> Result<()> {
    let root = tempfile::tempdir()?;
    let package = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let built = package.join("build/rtw");
    anyhow::ensure!(
        built.join("runtime.wasm").is_file(),
        "build the Chat WASM RTW before running this test"
    );
    let archive = root.path().join("chat.rtw");
    pack_directory(&built, &archive, RtwLimits::default())?;

    let home_path = root.path().join("home");
    let home = HostHome::open(&home_path)?;
    let world = home.create_world()?;
    let imported = home.install_local_rtw(&archive, Some(false))?;
    home.select_world_stored_rtw(world.id, &imported.activation.digest, Some(true))?;

    let conversation = ItemId::new();
    let message = ItemId::new();
    let relation = ItemId::new();
    let principal = PrincipalId::new();
    let mut host = HostRuntime::start(&home)?;
    host.activate_world(&home, world.id)?;

    host.submit_world_command(
        world.id,
        WorldCommand::new(
            key(CREATE_CONVERSATION_COMMAND)?,
            principal,
            json!({"conversation_id": conversation, "title": "Real WASM Chat"}),
        ),
    )?;
    host.submit_world_command(
        world.id,
        WorldCommand::new(
            key(SEND_MESSAGE_COMMAND)?,
            principal,
            json!({
                "conversation_id": conversation,
                "message_id": message,
                "relation_id": relation,
                "text": "Hello through Host"
            }),
        ),
    )?;
    host.shutdown()?;
    drop(home);

    let restarted_home = HostHome::open(&home_path)?;
    let db = restarted_home
        .root()
        .join("worlds")
        .join(world.id.to_string())
        .join("world.sqlite");
    let storage = SqliteWorldStorage::open(&db)?;
    assert_eq!(
        storage
            .load_property(conversation, &key(CONVERSATION_PROPERTY)?)?
            .context("conversation must be committed")?
            .data(),
        &json!({"title": "Real WASM Chat"})
    );
    assert_eq!(
        storage
            .load_property(message, &key(MESSAGE_PROPERTY)?)?
            .context("message must survive Host restart")?
            .data(),
        &json!({"role": "user", "text": "Hello through Host"})
    );
    assert!(storage.load_item(relation)?.is_some());
    drop(storage);

    let second = ItemId::new();
    let mut restarted = HostRuntime::start(&restarted_home)?;
    restarted.activate_world(&restarted_home, world.id)?;
    restarted.submit_world_command(
        world.id,
        WorldCommand::new(
            key(SEND_MESSAGE_COMMAND)?,
            principal,
            json!({
                "conversation_id": conversation,
                "message_id": second,
                "relation_id": ItemId::new(),
                "text": "Second message after restart"
            }),
        ),
    )?;
    restarted.shutdown()?;
    let storage = SqliteWorldStorage::open(&db)?;
    assert_eq!(
        storage
            .load_property(second, &key(MESSAGE_PROPERTY)?)?
            .context("new message must commit after restart")?
            .data(),
        &json!({"role": "user", "text": "Second message after restart"})
    );
    Ok(())
}

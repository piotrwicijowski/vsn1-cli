#![allow(dead_code)]

use std::path::Path;

use crate::protocol::LuaValue;
use crate::runtime::{Result, RuntimeError, RuntimeEvaluateReport, TransportRuntimeSlotReader};
use crate::runtime_bundle::OwnedRuntimeSlot;
use crate::targeting::ResolvedTarget;
use crate::transport::SerialTransport;

const READ_CHUNK_SIZE: usize = 50;
const WRITE_CHUNK_SIZE: usize = 50;

pub(crate) trait ModuleFileEvaluator {
    fn evaluate_lua(&mut self, target: ResolvedTarget, lua: &str) -> Result<RuntimeEvaluateReport>;
}

impl<T> ModuleFileEvaluator for TransportRuntimeSlotReader<T>
where
    T: SerialTransport,
{
    fn evaluate_lua(&mut self, target: ResolvedTarget, lua: &str) -> Result<RuntimeEvaluateReport> {
        Self::evaluate_lua(self, target, lua)
    }
}

pub(crate) fn derive_owned_slot_module_file_path(slot: &OwnedRuntimeSlot) -> Result<String> {
    if !Path::new(&slot.asset)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("lua"))
    {
        return Err(RuntimeError::unexpected_response(format!(
            "module-files provisioning currently supports only runtime-owned .lua event files; slot {} uses asset {}",
            slot.name, slot.asset
        )));
    }

    Ok(slot.derived_module_file_path())
}

pub(crate) fn read_owned_slot_module_file<E>(
    evaluator: &mut E,
    target: ResolvedTarget,
    slot: &OwnedRuntimeSlot,
) -> Result<Option<String>>
where
    E: ModuleFileEvaluator,
{
    let path = derive_owned_slot_module_file_path(slot)?;
    let mut content = String::new();
    let mut offset = 0usize;

    loop {
        let report = evaluator.evaluate_lua(target, &build_read_chunk_lua(&path, offset))?;
        let (exists, chunk) = decode_read_chunk_response(&path, &report.values)?;

        if !exists {
            if offset == 0 {
                return Ok(None);
            }

            return Err(RuntimeError::unexpected_response(format!(
                "module file {} disappeared while it was being read back",
                path
            )));
        }

        offset += chunk.len();
        content.push_str(&chunk);

        if chunk.len() < READ_CHUNK_SIZE {
            return Ok(Some(content));
        }
    }
}

pub(crate) fn write_owned_slot_module_file_atomic<E>(
    evaluator: &mut E,
    target: ResolvedTarget,
    slot: &OwnedRuntimeSlot,
    content: &str,
) -> Result<()>
where
    E: ModuleFileEvaluator,
{
    let path = derive_owned_slot_module_file_path(slot)?;
    let tmp_path = format!("{path}.tmp");

    for (index, chunk) in chunk_bytes(content.as_bytes(), WRITE_CHUNK_SIZE)
        .into_iter()
        .enumerate()
    {
        let mode = if index == 0 { "w" } else { "a" };
        let report =
            evaluator.evaluate_lua(target, &build_write_chunk_lua(&tmp_path, mode, chunk))?;
        decode_boolean_response(
            &format!("module file chunk write for {}", tmp_path),
            &report.values,
        )?;
    }

    let report = evaluator.evaluate_lua(target, &build_commit_write_lua(&tmp_path, &path))?;
    decode_boolean_response(
        &format!("module file rename from {} to {}", tmp_path, path),
        &report.values,
    )?;

    Ok(())
}

pub(crate) fn clear_owned_slot_module_file<E>(
    evaluator: &mut E,
    target: ResolvedTarget,
    slot: &OwnedRuntimeSlot,
) -> Result<()>
where
    E: ModuleFileEvaluator,
{
    let path = derive_owned_slot_module_file_path(slot)?;
    let report = evaluator.evaluate_lua(target, &build_clear_file_lua(&path))?;
    decode_boolean_response(&format!("module file clear for {}", path), &report.values)?;
    Ok(())
}

fn build_read_chunk_lua(path: &str, offset: usize) -> String {
    format!(
        "local f=io.open({path},\"r\") if not f then return false,nil end f:seek(\"set\",{offset}) local c=f:read({READ_CHUNK_SIZE}) f:close() collectgarbage(\"collect\") if c==nil then c=\"\" end return true,c",
        path = lua_string_literal(path.as_bytes()),
        offset = offset,
    )
}

fn build_write_chunk_lua(path: &str, mode: &str, chunk: &[u8]) -> String {
    format!(
        "local f=io.open({path},{mode}) if not f then return false end f:write({chunk}) f:close() collectgarbage(\"collect\") return true",
        path = lua_string_literal(path.as_bytes()),
        mode = lua_string_literal(mode.as_bytes()),
        chunk = lua_string_literal(chunk),
    )
}

fn build_commit_write_lua(tmp_path: &str, path: &str) -> String {
    format!(
        "os.remove({path}) return os.rename({tmp_path},{path})",
        tmp_path = lua_string_literal(tmp_path.as_bytes()),
        path = lua_string_literal(path.as_bytes()),
    )
}

fn build_clear_file_lua(path: &str) -> String {
    format!(
        "os.remove({path}) os.remove({tmp_path}) return true",
        path = lua_string_literal(path.as_bytes()),
        tmp_path = lua_string_literal(format!("{path}.tmp").as_bytes()),
    )
}

fn decode_read_chunk_response(path: &str, values: &[LuaValue]) -> Result<(bool, String)> {
    match values {
        [LuaValue::Boolean(false), LuaValue::Nil] => Ok((false, String::new())),
        [LuaValue::Boolean(true), LuaValue::String(chunk)] => Ok((true, chunk.clone())),
        _ => Err(RuntimeError::unexpected_response(format!(
            "module file read for {} returned an unexpected EVALUATE value shape",
            path
        ))),
    }
}

fn decode_boolean_response(operation: &str, values: &[LuaValue]) -> Result<()> {
    match values {
        [LuaValue::Boolean(true)] => Ok(()),
        [LuaValue::Boolean(false)] => Err(RuntimeError::unexpected_response(format!(
            "{operation} returned false"
        ))),
        _ => Err(RuntimeError::unexpected_response(format!(
            "{operation} returned an unexpected EVALUATE value shape"
        ))),
    }
}

fn chunk_bytes(bytes: &[u8], chunk_size: usize) -> Vec<&[u8]> {
    if bytes.is_empty() {
        vec![&[]]
    } else {
        bytes.chunks(chunk_size).collect()
    }
}

fn lua_string_literal(bytes: &[u8]) -> String {
    let mut output = String::from("\"");

    for &byte in bytes {
        match byte {
            b'\\' => output.push_str("\\\\"),
            b'"' => output.push_str("\\\""),
            b'\n' => output.push_str("\\n"),
            b'\r' => output.push_str("\\r"),
            b'\t' => output.push_str("\\t"),
            0x20..=0x7e => output.push(byte as char),
            _ => output.push_str(&format!("\\x{byte:02x}")),
        }
    }

    output.push('"');
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::protocol::GridTarget;

    #[derive(Default)]
    struct RecordingEvaluator {
        scripts: Vec<String>,
        responses: Vec<RuntimeEvaluateReport>,
    }

    impl RecordingEvaluator {
        fn push_response(&mut self, values: Vec<LuaValue>) {
            self.responses.push(RuntimeEvaluateReport {
                source_target: GridTarget::new(0, 0),
                values,
            });
        }
    }

    impl ModuleFileEvaluator for RecordingEvaluator {
        fn evaluate_lua(
            &mut self,
            _target: ResolvedTarget,
            lua: &str,
        ) -> Result<RuntimeEvaluateReport> {
            self.scripts.push(lua.to_string());

            if self.responses.is_empty() {
                return Err(RuntimeError::unexpected_response(
                    "test evaluator ran out of queued responses",
                ));
            }

            Ok(self.responses.remove(0))
        }
    }

    fn fixture_slot() -> OwnedRuntimeSlot {
        OwnedRuntimeSlot {
            name: "lcd-draw".to_string(),
            page: 0,
            element: 13,
            event: 8,
            asset: "lcd-draw.lua".to_string(),
            install_order: 20,
        }
    }

    #[test]
    fn derives_lowercase_hex_module_file_paths_from_owned_slots() {
        let path = derive_owned_slot_module_file_path(&fixture_slot()).unwrap();

        assert_eq!(path, "/00/0d/08.lua");
    }

    #[test]
    fn rejects_non_lua_assets_for_module_file_provisioning() {
        let mut slot = fixture_slot();
        slot.asset = "lcd-draw.txt".to_string();

        let error = derive_owned_slot_module_file_path(&slot).unwrap_err();

        assert_eq!(
            error.to_string(),
            "runtime inspection failed: module-files provisioning currently supports only runtime-owned .lua event files; slot lcd-draw uses asset lcd-draw.txt"
        );
    }

    #[test]
    fn reads_module_files_back_in_multiple_chunks() {
        let mut evaluator = RecordingEvaluator::default();
        evaluator.push_response(vec![
            LuaValue::Boolean(true),
            LuaValue::String("a".repeat(50)),
        ]);
        evaluator.push_response(vec![
            LuaValue::Boolean(true),
            LuaValue::String("tail".to_string()),
        ]);

        let content = read_owned_slot_module_file(
            &mut evaluator,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &fixture_slot(),
        )
        .unwrap();

        assert_eq!(content, Some(format!("{}tail", "a".repeat(50))));
        assert_eq!(evaluator.scripts.len(), 2);
        assert!(evaluator.scripts[0].contains("io.open(\"/00/0d/08.lua\",\"r\")"));
        assert!(evaluator.scripts[0].contains("f:seek(\"set\",0)"));
        assert!(evaluator.scripts[1].contains("f:seek(\"set\",50)"));
        assert!(evaluator.scripts[1].contains("f:read(50)"));
    }

    #[test]
    fn returns_none_when_module_file_is_missing() {
        let mut evaluator = RecordingEvaluator::default();
        evaluator.push_response(vec![LuaValue::Boolean(false), LuaValue::Nil]);

        let content = read_owned_slot_module_file(
            &mut evaluator,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &fixture_slot(),
        )
        .unwrap();

        assert_eq!(content, None);
    }

    #[test]
    fn writes_module_files_in_chunked_temp_file_appends_then_renames() {
        let mut evaluator = RecordingEvaluator::default();
        evaluator.push_response(vec![LuaValue::Boolean(true)]);
        evaluator.push_response(vec![LuaValue::Boolean(true)]);
        evaluator.push_response(vec![LuaValue::Boolean(true)]);

        let content = format!("{}tail", "a".repeat(50));
        write_owned_slot_module_file_atomic(
            &mut evaluator,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &fixture_slot(),
            &content,
        )
        .unwrap();

        assert_eq!(evaluator.scripts.len(), 3);
        assert!(evaluator.scripts[0].contains("io.open(\"/00/0d/08.lua.tmp\",\"w\")"));
        assert!(evaluator.scripts[0].contains(&format!("f:write(\"{}\")", "a".repeat(50))));
        assert!(evaluator.scripts[1].contains("io.open(\"/00/0d/08.lua.tmp\",\"a\")"));
        assert!(evaluator.scripts[1].contains("f:write(\"tail\")"));
        assert!(evaluator.scripts[2].contains("os.rename(\"/00/0d/08.lua.tmp\",\"/00/0d/08.lua\")"));
    }

    #[test]
    fn writes_empty_module_files_by_creating_an_empty_temp_file() {
        let mut evaluator = RecordingEvaluator::default();
        evaluator.push_response(vec![LuaValue::Boolean(true)]);
        evaluator.push_response(vec![LuaValue::Boolean(true)]);

        write_owned_slot_module_file_atomic(
            &mut evaluator,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &fixture_slot(),
            "",
        )
        .unwrap();

        assert_eq!(evaluator.scripts.len(), 2);
        assert!(evaluator.scripts[0].contains("io.open(\"/00/0d/08.lua.tmp\",\"w\")"));
        assert!(evaluator.scripts[0].contains("f:write(\"\")"));
    }

    #[test]
    fn clears_module_files_and_their_stale_temp_files() {
        let mut evaluator = RecordingEvaluator::default();
        evaluator.push_response(vec![LuaValue::Boolean(true)]);

        clear_owned_slot_module_file(
            &mut evaluator,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &fixture_slot(),
        )
        .unwrap();

        assert_eq!(evaluator.scripts.len(), 1);
        assert!(evaluator.scripts[0].contains("os.remove(\"/00/0d/08.lua\")"));
        assert!(evaluator.scripts[0].contains("os.remove(\"/00/0d/08.lua.tmp\")"));
    }

    #[test]
    fn escapes_non_printable_write_bytes_for_lua() {
        let escaped = lua_string_literal(&[0, b'"', b'\\', b'\n', 0x7f]);

        assert_eq!(escaped, "\"\\x00\\\"\\\\\\n\\x7f\"");
    }
}

#![allow(dead_code)]

use std::path::Path;

use crate::debug;
use crate::protocol::LuaValue;
use crate::runtime::{
    Result, RuntimeError, RuntimeEvaluateReport, RuntimeSlotRead, TransportRuntimeSlotReader,
};
use crate::runtime_bundle::{RuntimeOwnedAsset, RuntimeOwnedAssetLocation};
use crate::targeting::ResolvedTarget;
use crate::transport::SerialTransport;

// Keep module-file reads aligned with Grid Editor's proven 50-byte chunking.
// Larger chunks were attractive for throughput, but hardware validation showed
// missed EVALUATE reports on larger runtime-module transfers.
const READ_CHUNK_SIZE: usize = 50;

// Keep writes at the same conservative chunk size for parity with the working
// Grid Editor file-manager path.
const WRITE_CHUNK_SIZE: usize = 50;

// File-manager reads are idempotent, so a small retry budget is safe when the
// firmware drops an EVALUATE reply for a chunk read.
const READ_EVALUATE_ATTEMPTS: usize = 3;

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

pub(crate) fn derive_owned_module_file_path(owned: &RuntimeOwnedAsset) -> Result<String> {
    if !Path::new(&owned.asset)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("lua"))
    {
        return Err(RuntimeError::unexpected_response(format!(
            "module-files provisioning currently supports only runtime-owned .lua event files; slot {} uses asset {}",
            owned.name, owned.asset
        )));
    }

    match &owned.location {
        RuntimeOwnedAssetLocation::Slot(slot) => Ok(slot.derived_module_file_path()),
        RuntimeOwnedAssetLocation::File(file) => Ok(file.path.clone()),
    }
}

pub(crate) fn read_owned_slot_module_file<E>(
    evaluator: &mut E,
    target: ResolvedTarget,
    owned: &RuntimeOwnedAsset,
) -> Result<Option<RuntimeSlotRead>>
where
    E: ModuleFileEvaluator,
{
    let path = derive_owned_module_file_path(owned)?;
    read_module_file(evaluator, target, &path)
}

pub(crate) fn read_owned_slot_module_file_with_progress<E>(
    evaluator: &mut E,
    target: ResolvedTarget,
    owned: &RuntimeOwnedAsset,
    on_step: &mut dyn FnMut(),
) -> Result<Option<RuntimeSlotRead>>
where
    E: ModuleFileEvaluator,
{
    let path = derive_owned_module_file_path(owned)?;
    read_module_file_with_progress(evaluator, target, &path, on_step)
}

pub(crate) fn read_module_file<E>(
    evaluator: &mut E,
    target: ResolvedTarget,
    path: &str,
) -> Result<Option<RuntimeSlotRead>>
where
    E: ModuleFileEvaluator,
{
    read_module_file_with_progress(evaluator, target, path, &mut || {})
}

pub(crate) fn read_module_file_with_progress<E>(
    evaluator: &mut E,
    target: ResolvedTarget,
    path: &str,
    on_step: &mut dyn FnMut(),
) -> Result<Option<RuntimeSlotRead>>
where
    E: ModuleFileEvaluator,
{
    let mut content = String::new();
    let mut offset = 0usize;
    let mut source_target = None;

    loop {
        let report = evaluate_module_file_read_chunk(evaluator, target, &path, offset)?;
        match source_target {
            Some(expected_target) if report.source_target != expected_target => {
                return Err(RuntimeError::unexpected_response(format!(
                    "module file {} responded from multiple module targets while it was being read back",
                    path
                )));
            }
            None => source_target = Some(report.source_target),
            _ => {}
        }
        let (exists, chunk) = decode_read_chunk_response(&path, &report.values)?;

        if !exists {
            if offset == 0 {
                on_step();
                return Ok(None);
            }

            return Err(RuntimeError::unexpected_response(format!(
                "module file {} disappeared while it was being read back",
                path
            )));
        }

        offset += chunk.len();
        content.push_str(&chunk);
        on_step();

        if chunk.len() < READ_CHUNK_SIZE {
            return Ok(Some(RuntimeSlotRead {
                source_target: source_target
                    .expect("module file read should observe a source target"),
                content,
            }));
        }
    }
}

fn evaluate_module_file_read_chunk<E>(
    evaluator: &mut E,
    target: ResolvedTarget,
    path: &str,
    offset: usize,
) -> Result<RuntimeEvaluateReport>
where
    E: ModuleFileEvaluator,
{
    let lua = build_read_chunk_lua(path, offset);

    for attempt in 1..=READ_EVALUATE_ATTEMPTS {
        match evaluator.evaluate_lua(target, &lua) {
            Ok(report) => return Ok(report),
            Err(error)
                if is_missing_evaluate_report(&error) && attempt < READ_EVALUATE_ATTEMPTS =>
            {
                debug::log(
                    "module-files",
                    format!(
                        "retrying read for {} at offset {} after missing EVALUATE report (attempt {}/{})",
                        path, offset, attempt + 1, READ_EVALUATE_ATTEMPTS
                    ),
                );
            }
            Err(error) => return Err(error),
        }
    }

    unreachable!("module file read retry loop should return or error")
}

fn is_missing_evaluate_report(error: &RuntimeError) -> bool {
    matches!(
        error,
        RuntimeError::UnexpectedResponse { message }
            if message == "no EVALUATE report was observed in read-back"
    )
}

pub(crate) fn module_file_read_step_estimate(content: &str) -> usize {
    chunk_bytes(content.as_bytes(), READ_CHUNK_SIZE)
        .len()
        .max(1)
}

pub(crate) fn write_owned_slot_module_file_atomic<E>(
    evaluator: &mut E,
    target: ResolvedTarget,
    owned: &RuntimeOwnedAsset,
    content: &str,
) -> Result<()>
where
    E: ModuleFileEvaluator,
{
    let path = derive_owned_module_file_path(owned)?;
    write_module_file_atomic(evaluator, target, &path, content)
}

pub(crate) fn write_owned_slot_module_file_atomic_with_progress<E>(
    evaluator: &mut E,
    target: ResolvedTarget,
    owned: &RuntimeOwnedAsset,
    content: &str,
    on_step: &mut dyn FnMut(),
) -> Result<()>
where
    E: ModuleFileEvaluator,
{
    let path = derive_owned_module_file_path(owned)?;
    write_module_file_atomic_with_progress(evaluator, target, &path, content, on_step)
}

pub(crate) fn write_module_file_atomic<E>(
    evaluator: &mut E,
    target: ResolvedTarget,
    path: &str,
    content: &str,
) -> Result<()>
where
    E: ModuleFileEvaluator,
{
    write_module_file_atomic_with_progress(evaluator, target, path, content, &mut || {})
}

pub(crate) fn write_module_file_atomic_with_progress<E>(
    evaluator: &mut E,
    target: ResolvedTarget,
    path: &str,
    content: &str,
    on_step: &mut dyn FnMut(),
) -> Result<()>
where
    E: ModuleFileEvaluator,
{
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
        on_step();
    }

    let report = evaluator.evaluate_lua(target, &build_commit_write_lua(&tmp_path, &path))?;
    decode_boolean_response(
        &format!("module file rename from {} to {}", tmp_path, path),
        &report.values,
    )?;
    on_step();

    Ok(())
}

pub(crate) fn module_file_write_step_count(content: &str) -> usize {
    chunk_bytes(content.as_bytes(), WRITE_CHUNK_SIZE).len() + 1
}

pub(crate) fn clear_owned_slot_module_file<E>(
    evaluator: &mut E,
    target: ResolvedTarget,
    owned: &RuntimeOwnedAsset,
) -> Result<()>
where
    E: ModuleFileEvaluator,
{
    let path = derive_owned_module_file_path(owned)?;
    clear_module_file(evaluator, target, &path)
}

pub(crate) fn clear_module_file<E>(
    evaluator: &mut E,
    target: ResolvedTarget,
    path: &str,
) -> Result<()>
where
    E: ModuleFileEvaluator,
{
    let report = evaluator.evaluate_lua(target, &build_clear_file_lua(path))?;
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
        _ => {
            debug::log(
                "module-files",
                format!("unexpected read response for {}: values={:?}", path, values),
            );
            Err(RuntimeError::unexpected_response(format!(
                "module file read for {} returned an unexpected EVALUATE value shape: {:?}",
                path, values
            )))
        }
    }
}

fn decode_boolean_response(operation: &str, values: &[LuaValue]) -> Result<()> {
    match values {
        [LuaValue::Boolean(true)] => Ok(()),
        [LuaValue::Boolean(true), LuaValue::String(extra)] if extra.is_empty() => Ok(()),
        [LuaValue::Boolean(true), LuaValue::Nil] => Ok(()),
        [LuaValue::Boolean(false)] => Err(RuntimeError::unexpected_response(format!(
            "{operation} returned false"
        ))),
        _ => {
            debug::log(
                "module-files",
                format!(
                    "unexpected boolean response for {}: values={:?}",
                    operation, values
                ),
            );
            Err(RuntimeError::unexpected_response(format!(
                "{operation} returned an unexpected EVALUATE value shape: {:?}",
                values
            )))
        }
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

    fn fixture_slot_asset() -> RuntimeOwnedAsset {
        RuntimeOwnedAsset {
            name: "lcd-draw".to_string(),
            asset: "lcd-draw.lua".to_string(),
            install_order: 20,
            location: RuntimeOwnedAssetLocation::Slot(crate::runtime_bundle::OwnedRuntimeSlot {
                name: "lcd-draw".to_string(),
                page: 0,
                element: 13,
                event: 8,
                asset: "lcd-draw.lua".to_string(),
                install_order: 20,
            }),
        }
    }

    #[test]
    fn derives_lowercase_hex_module_file_paths_from_owned_slots() {
        let path = derive_owned_module_file_path(&fixture_slot_asset()).unwrap();

        assert_eq!(path, "/00/0d/08.cfg");
    }

    #[test]
    fn rejects_non_lua_assets_for_module_file_provisioning() {
        let mut owned = fixture_slot_asset();
        owned.asset = "lcd-draw.txt".to_string();

        let error = derive_owned_module_file_path(&owned).unwrap_err();

        assert_eq!(
            error.to_string(),
            "runtime inspection failed: module-files provisioning currently supports only runtime-owned .lua event files; slot lcd-draw uses asset lcd-draw.txt"
        );
    }

    #[test]
    fn uses_explicit_helper_module_file_paths_when_present() {
        let owned = RuntimeOwnedAsset {
            name: "helper".to_string(),
            asset: "helper.lua".to_string(),
            install_order: 10,
            location: RuntimeOwnedAssetLocation::File(crate::runtime_bundle::OwnedRuntimeFile {
                name: "helper".to_string(),
                path: "/vsn1_media_draw.lua".to_string(),
                asset: "helper.lua".to_string(),
                install_order: 10,
            }),
        };

        let path = derive_owned_module_file_path(&owned).unwrap();

        assert_eq!(path, "/vsn1_media_draw.lua");
    }

    #[test]
    fn reads_module_files_back_in_multiple_chunks() {
        let mut evaluator = RecordingEvaluator::default();
        evaluator.push_response(vec![
            LuaValue::Boolean(true),
            LuaValue::String("a".repeat(READ_CHUNK_SIZE)),
        ]);
        evaluator.push_response(vec![
            LuaValue::Boolean(true),
            LuaValue::String("tail".to_string()),
        ]);

        let content = read_owned_slot_module_file(
            &mut evaluator,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &fixture_slot_asset(),
        )
        .unwrap();

        let content = content.unwrap();
        assert_eq!(
            content.content,
            format!("{}tail", "a".repeat(READ_CHUNK_SIZE))
        );
        assert_eq!(content.source_target, GridTarget::new(0, 0));
        assert_eq!(evaluator.scripts.len(), 2);
        assert!(evaluator.scripts[0].contains("io.open(\"/00/0d/08.cfg\",\"r\")"));
        assert!(evaluator.scripts[0].contains("f:seek(\"set\",0)"));
        assert!(evaluator.scripts[1].contains(&format!("f:seek(\"set\",{READ_CHUNK_SIZE})")));
        assert!(evaluator.scripts[1].contains(&format!("f:read({READ_CHUNK_SIZE})")));
    }

    #[test]
    fn reads_arbitrary_module_file_paths_back_in_multiple_chunks() {
        let mut evaluator = RecordingEvaluator::default();
        evaluator.push_response(vec![
            LuaValue::Boolean(true),
            LuaValue::String("a".repeat(READ_CHUNK_SIZE)),
        ]);
        evaluator.push_response(vec![
            LuaValue::Boolean(true),
            LuaValue::String("tail".to_string()),
        ]);

        let content = read_module_file(
            &mut evaluator,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            "/vsn1-cli-runtime-manifest.toml",
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            content.content,
            format!("{}tail", "a".repeat(READ_CHUNK_SIZE))
        );
        assert!(evaluator.scripts[0].contains("io.open(\"/vsn1-cli-runtime-manifest.toml\",\"r\")"));
    }

    #[test]
    fn returns_none_when_module_file_is_missing() {
        let mut evaluator = RecordingEvaluator::default();
        evaluator.push_response(vec![LuaValue::Boolean(false), LuaValue::Nil]);

        let content = read_owned_slot_module_file(
            &mut evaluator,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &fixture_slot_asset(),
        )
        .unwrap();

        assert_eq!(content, None);
    }

    #[test]
    fn reports_progress_for_each_read_chunk() {
        let mut evaluator = RecordingEvaluator::default();
        evaluator.push_response(vec![
            LuaValue::Boolean(true),
            LuaValue::String("a".repeat(READ_CHUNK_SIZE)),
        ]);
        evaluator.push_response(vec![
            LuaValue::Boolean(true),
            LuaValue::String("tail".to_string()),
        ]);

        let mut steps = 0;
        let content = read_owned_slot_module_file_with_progress(
            &mut evaluator,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &fixture_slot_asset(),
            &mut || steps += 1,
        )
        .unwrap();

        assert_eq!(
            content.unwrap().content,
            format!("{}tail", "a".repeat(READ_CHUNK_SIZE))
        );
        assert_eq!(steps, 2);
    }

    #[test]
    fn retries_module_file_reads_after_missing_evaluate_reports() {
        let mut evaluator = RecordingEvaluator::default();
        evaluator.push_response(vec![
            LuaValue::Boolean(true),
            LuaValue::String("tail".to_string()),
        ]);

        let mut attempts = 0usize;
        let content = read_module_file_with_progress(
            &mut FailingReadEvaluator {
                failures_before_success: 2,
                inner: &mut evaluator,
                attempts: &mut attempts,
            },
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            "/helper.lua",
            &mut || {},
        )
        .unwrap()
        .unwrap();

        assert_eq!(attempts, 3);
        assert_eq!(content.content, "tail");
    }

    #[test]
    fn reports_progress_when_module_file_is_missing() {
        let mut evaluator = RecordingEvaluator::default();
        evaluator.push_response(vec![LuaValue::Boolean(false), LuaValue::Nil]);

        let mut steps = 0;
        let content = read_owned_slot_module_file_with_progress(
            &mut evaluator,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &fixture_slot_asset(),
            &mut || steps += 1,
        )
        .unwrap();

        assert_eq!(content, None);
        assert_eq!(steps, 1);
    }

    #[test]
    fn writes_module_files_in_chunked_temp_file_appends_then_renames() {
        let mut evaluator = RecordingEvaluator::default();
        evaluator.push_response(vec![LuaValue::Boolean(true)]);
        evaluator.push_response(vec![LuaValue::Boolean(true)]);
        evaluator.push_response(vec![LuaValue::Boolean(true)]);

        let content = format!("{}tail", "a".repeat(WRITE_CHUNK_SIZE));
        write_owned_slot_module_file_atomic(
            &mut evaluator,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &fixture_slot_asset(),
            &content,
        )
        .unwrap();

        assert_eq!(evaluator.scripts.len(), 3);
        assert!(evaluator.scripts[0].contains("io.open(\"/00/0d/08.cfg.tmp\",\"w\")"));
        assert!(evaluator.scripts[0]
            .contains(&format!("f:write(\"{}\")", "a".repeat(WRITE_CHUNK_SIZE))));
        assert!(evaluator.scripts[1].contains("io.open(\"/00/0d/08.cfg.tmp\",\"a\")"));
        assert!(evaluator.scripts[1].contains("f:write(\"tail\")"));
        assert!(evaluator.scripts[2].contains("os.rename(\"/00/0d/08.cfg.tmp\",\"/00/0d/08.cfg\")"));
    }

    #[test]
    fn writes_arbitrary_module_file_paths_in_chunked_temp_file_appends_then_renames() {
        let mut evaluator = RecordingEvaluator::default();
        evaluator.push_response(vec![LuaValue::Boolean(true)]);
        evaluator.push_response(vec![LuaValue::Boolean(true)]);

        write_module_file_atomic(
            &mut evaluator,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            "/vsn1-cli-runtime-manifest.toml",
            "name = 'test'",
        )
        .unwrap();

        assert_eq!(evaluator.scripts.len(), 2);
        assert!(
            evaluator.scripts[0].contains("io.open(\"/vsn1-cli-runtime-manifest.toml.tmp\",\"w\")")
        );
        assert!(evaluator.scripts[1].contains(
            "os.rename(\"/vsn1-cli-runtime-manifest.toml.tmp\",\"/vsn1-cli-runtime-manifest.toml\")"
        ));
    }

    #[test]
    fn writes_empty_module_files_by_creating_an_empty_temp_file() {
        let mut evaluator = RecordingEvaluator::default();
        evaluator.push_response(vec![LuaValue::Boolean(true)]);
        evaluator.push_response(vec![LuaValue::Boolean(true)]);

        write_owned_slot_module_file_atomic(
            &mut evaluator,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &fixture_slot_asset(),
            "",
        )
        .unwrap();

        assert_eq!(evaluator.scripts.len(), 2);
        assert!(evaluator.scripts[0].contains("io.open(\"/00/0d/08.cfg.tmp\",\"w\")"));
        assert!(evaluator.scripts[0].contains("f:write(\"\")"));
    }

    #[test]
    fn reports_progress_for_each_chunk_write_and_commit() {
        let mut evaluator = RecordingEvaluator::default();
        evaluator.push_response(vec![LuaValue::Boolean(true)]);
        evaluator.push_response(vec![LuaValue::Boolean(true)]);
        evaluator.push_response(vec![LuaValue::Boolean(true)]);

        let mut steps = 0;
        let content = format!("{}tail", "a".repeat(WRITE_CHUNK_SIZE));
        write_owned_slot_module_file_atomic_with_progress(
            &mut evaluator,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &fixture_slot_asset(),
            &content,
            &mut || steps += 1,
        )
        .unwrap();

        assert_eq!(steps, module_file_write_step_count(&content));
    }

    #[test]
    fn accepts_boolean_success_with_trailing_empty_string() {
        decode_boolean_response(
            "module file chunk write for /helper.lua.tmp",
            &[LuaValue::Boolean(true), LuaValue::String(String::new())],
        )
        .unwrap();
    }

    #[test]
    fn clears_module_files_and_their_stale_temp_files() {
        let mut evaluator = RecordingEvaluator::default();
        evaluator.push_response(vec![LuaValue::Boolean(true)]);

        clear_owned_slot_module_file(
            &mut evaluator,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            &fixture_slot_asset(),
        )
        .unwrap();

        assert_eq!(evaluator.scripts.len(), 1);
        assert!(evaluator.scripts[0].contains("os.remove(\"/00/0d/08.cfg\")"));
        assert!(evaluator.scripts[0].contains("os.remove(\"/00/0d/08.cfg.tmp\")"));
    }

    #[test]
    fn clears_arbitrary_module_files_and_their_stale_temp_files() {
        let mut evaluator = RecordingEvaluator::default();
        evaluator.push_response(vec![LuaValue::Boolean(true)]);

        clear_module_file(
            &mut evaluator,
            ResolvedTarget::Explicit(GridTarget::new(0, 0)),
            "/vsn1-cli-runtime-manifest.toml",
        )
        .unwrap();

        assert_eq!(evaluator.scripts.len(), 1);
        assert!(evaluator.scripts[0].contains("os.remove(\"/vsn1-cli-runtime-manifest.toml\")"));
        assert!(evaluator.scripts[0].contains("os.remove(\"/vsn1-cli-runtime-manifest.toml.tmp\")"));
    }

    #[test]
    fn escapes_non_printable_write_bytes_for_lua() {
        let escaped = lua_string_literal(&[0, b'"', b'\\', b'\n', 0x7f]);

        assert_eq!(escaped, "\"\\x00\\\"\\\\\\n\\x7f\"");
    }

    struct FailingReadEvaluator<'a> {
        failures_before_success: usize,
        inner: &'a mut RecordingEvaluator,
        attempts: &'a mut usize,
    }

    impl ModuleFileEvaluator for FailingReadEvaluator<'_> {
        fn evaluate_lua(
            &mut self,
            target: ResolvedTarget,
            lua: &str,
        ) -> Result<RuntimeEvaluateReport> {
            *self.attempts += 1;

            if self.failures_before_success > 0 {
                self.failures_before_success -= 1;
                return Err(RuntimeError::unexpected_response(
                    "no EVALUATE report was observed in read-back",
                ));
            }

            self.inner.evaluate_lua(target, lua)
        }
    }
}

-- vsn1-cli runtime bundle 2026-06-12-screen-first.1
--
-- This draw slot intentionally stays minimal until the standalone renderer is
-- validated against real hardware. For now the bundle contract asserts that
-- the LCD init slot was installed and leaves visible rendering to later steps.

if type(vsn1_cli_runtime_identity) ~= "function" then
  error("vsn1-cli runtime init is missing")
end

if type(vsn1_cli_state) ~= "table" then
  error("vsn1-cli runtime state is missing")
end

return vsn1_cli_runtime_identity()

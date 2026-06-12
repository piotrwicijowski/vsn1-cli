# VSN1 CLI Planning: Layered Display Semantic Decisions

## Source

User answers collected interactively on 2026-06-12.

## Confirmed decisions

1. **Fallback rule:** highest active lower layer.
   - `fast` should fall back to `slow` if `slow` is still active.
   - Otherwise `fast` falls back to `persistent`.
   - `slow` falls back to `persistent`.

2. **CLI activation behavior:** support both activation-only and set-and-activate flows.
   - There should be an explicit activation command.
   - The CLI should also support a combined flow where a temporary layer is updated and activated in one command.

3. **Reactivation rule:** restart timer.
   - Re-activating `slow` restarts its `5s` timer from now.
   - Re-activating `fast` restarts its `1s` timer from now.

4. **Curated field shape:** per-layer custom fields are allowed.
   - The three layers do not need to expose identical curated parameters.
   - The public API can intentionally keep `fast` and `slow` narrower than `persistent`.

5. **`screen clear` default behavior:** require explicit layer.
   - `screen clear` should not guess which layer to clear.
   - The caller should specify the target layer.

6. **Layer naming syntax for `screen set`:** layer prefix in the public field name, separated with a dot.
   - Examples:
     - `persistent.title`
     - `slow.message`
     - `fast.action`

## Implications

1. The device-side runtime needs independent active/until state for `slow` and `fast`.
2. Visibility resolution should be based on priority plus active timeout state, not just the last activation event.
3. The host-side field registry should likely be keyed by full public names such as `persistent.title` rather than a shared field enum alone.
4. Since per-layer fields may differ, `screen clear` must likely clear according to a layer-specific registry rather than one global field list.

## Likely command consequences

1. `screen activate slow`
2. `screen activate fast`
3. `screen set persistent.title="..." persistent.artist="..."`
4. `screen set slow.message="Disk almost full" --activate slow`
5. `screen clear slow`

## Confirmed combined activation shape

Combined set-and-activate behavior should use:

1. `screen set ... --activate slow`
2. `screen set ... --activate fast`

This keeps the command surface smaller and matches the main `screen set` model.

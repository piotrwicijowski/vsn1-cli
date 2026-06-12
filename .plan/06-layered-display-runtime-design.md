# VSN1 CLI Planning: Layered Display Runtime Design

## Source

User-provided design addition on 2026-06-12.

## Confirmed decisions

1. The runtime/profile should expose three display layers:
   - `persistent`
   - `slow`
   - `fast`

2. The `slow` and `fast` layers are explicitly activated by CLI command.

3. Once activated, a higher-priority layer becomes visible temporarily and then reverts automatically to a lower layer.

4. Current timeout policy:
   - `slow` reverts after `5s`
   - `fast` reverts after `1s`

5. Updating values in lower-priority layers must not make them visible while a higher-priority layer is active.

6. Parameter names should encode the target layer via prefix:
   - `persistent_*`
   - `slow_*`
   - `fast_*`

7. Intended usage model:
   - `persistent`: long-lived primary content such as track artist, title, and progress
   - `slow`: medium-priority notifications such as system notices
   - `fast`: short reaction feedback such as play, pause, or volume changes

## Architectural implications

1. The runtime can no longer be modeled as a single flat parameter state.
2. The device-side runtime likely needs:
   - separate stored state per layer
   - an active-layer selector
   - a timer or deadline concept for temporary layer visibility
   - redraw logic that renders the highest currently active layer

3. The host-side CLI likely needs separate concepts for:
   - updating layer state
   - activating a layer
   - possibly combined update-and-activate flows

4. The visible-screen model becomes priority-driven rather than last-write-wins.

## Open questions to resolve

1. Whether `persistent` is always the base fallback, or whether `fast` should revert to `slow` when `slow` is still active.
2. Whether activating `slow` or `fast` should optionally update values at the same time.
3. Whether repeated activations should extend the current timer or restart from now.
4. Whether `clear` should clear one layer or all layers.
5. Whether each layer supports the same parameter set or different curated fields.

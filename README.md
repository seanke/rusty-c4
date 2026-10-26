# rustyc4

A 3D viewer for [LikeC4](https://likec4.dev) architecture models in a Tron style, built
with [Bevy](https://bevyengine.org).

Every element is a block extruded from a neon grid floor, with its name painted on its top,
and relationships are glowing pipes routed between them at right angles; where two pipes
have to cross, one bridges over the other. There are no separate levels to switch between:
zoom in and any block that grows big enough rises into a platform, its title sliding to the
top edge as its children extrude out of it; zoom out and they sink away again. Pipes
leaving a platform run over its edge and down its side.

Dynamic views play as stories: pick one from the list and each step sends a pulse of light
along the pipe between its two blocks, in order, while the camera flies to each step and
everything not taking part dims.

## Running

```sh
cargo run                       # welcome screen: open a folder or file, or drop one in
cargo run -- path/to/project    # every .c4 / .likec4 file under a directory
cargo run -- path/to/model.c4   # a single file
cargo run -- --example          # the built-in example
```

Files are watched and reloaded on save.

## Controls

| Input                        | Action                                         |
| ---------------------------- | ---------------------------------------------- |
| Click a block                | Fly to it (it opens), or show its details if it has nothing inside |
| Click a pipe                 | Zoom into it: every relation it stands for     |
| Click the floor, Backspace, right-click | Close details, or zoom out a level |
| Scroll / pinch               | Zoom                                           |
| Drag, two-finger scroll      | Pan                                            |
| Right-drag                   | Orbit and tilt the camera                      |
| F, Home                      | Fit everything                                 |
| Space, ← →, Backspace        | While a dynamic view plays: pause, step, stop  |
| Esc                          | Menu: open another project, reload, quit       |

## What it reads

A forgiving parser for the drawing-relevant parts of the LikeC4 DSL: `specification` element
kinds and their `style` (shape, color), `model` elements in both `name = kind` and
`kind name` forms, nesting, `extend`, `group`, and relationships (`->`, `-[kind]->`,
`.kind`, `<->`, implicit `this`), and `dynamic view`s (steps, `<-` replies, `parallel`
blocks). Other views and `deployment` blocks are skipped; rustyc4 lays out each level itself.

Requires Rust 1.89 or newer.

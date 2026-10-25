# rustyc4

A 3D viewer for [LikeC4](https://likec4.dev) architecture models in a Tron style, built
with [Bevy](https://bevyengine.org).

Every element is a block extruded from a neon grid floor, and relationships are glowing
pipes routed between them at right angles; where two pipes have to cross, one bridges over
the other. Click a block and it rises into a platform while the camera flies in, and its
children extrude out of its top; everything else stays where it is, so the rest of the
model is always around you. Pipes leaving a platform run over its edge and down its side.
Jumps across the hierarchy walk the tree one level at a time.

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
| Click a block                | Open it, or zoom into its details if it has nothing inside |
| Click a pipe                 | Zoom into it: every relation it stands for     |
| Click the floor, Backspace, right-click | Close details, or go up a level |
| Click a block on another level | Walk over to it                              |
| Scroll / pinch               | Zoom                                           |
| Drag, two-finger scroll      | Pan                                            |
| Right-drag                   | Orbit and tilt the camera                      |
| F                            | Fit to view                                    |
| Esc                          | Menu: open another project, reload, quit       |

## What it reads

A forgiving parser for the drawing-relevant parts of the LikeC4 DSL: `specification` element
kinds and their `style` (shape, color), `model` elements in both `name = kind` and
`kind name` forms, nesting, `extend`, `group`, and relationships (`->`, `-[kind]->`,
`.kind`, `<->`, implicit `this`). `views` and `deployment` blocks are skipped; rustyc4 lays
out each level itself.

Requires Rust 1.89 or newer.

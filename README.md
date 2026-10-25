# rustyc4

A smooth, zoomable viewer for [LikeC4](https://likec4.dev) architecture models, built with
[Bevy](https://bevyengine.org).

Click into any element and the camera moves in: its card grows into a frame, its children
spring out of it, and the level around it is pushed back into the distance, smaller and
fainter but still there, so you always know where you are and what it connects to. Jumps
across the hierarchy walk the tree one level at a time. Relationships hang between cards
like heavy chains: they sway as the camera moves, and you can grab a card or a name tag and give
it a shake to see what is tied to what.

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
| Click a card                 | Open it, or zoom into its details if it has nothing inside |
| Click a relationship         | Zoom into it: every relation it stands for     |
| Click outside, Backspace, right-click | Close details, or go up a level |
| Click a faint element        | Walk over to it                                |
| Scroll / pinch               | Zoom                                           |
| Drag empty space, two-finger scroll | Pan                                     |
| Drag a card or a relationship's tag | Shake it about on its chains; it swings back |
| F                            | Fit to view                                    |
| Esc                          | Menu: open another project, reload, quit       |

## What it reads

A forgiving parser for the drawing-relevant parts of the LikeC4 DSL: `specification` element
kinds and their `style` (shape, color), `model` elements in both `name = kind` and
`kind name` forms, nesting, `extend`, `group`, and relationships (`->`, `-[kind]->`,
`.kind`, `<->`, implicit `this`). `views` and `deployment` blocks are skipped; rustyc4 lays
out each level itself.

Requires Rust 1.89 or newer.

# rustyc4

A smooth, zoomable viewer for [LikeC4](https://likec4.dev) architecture models, built with
[Bevy](https://bevyengine.org).

Click into any element and it opens in place: its card grows into a frame, its children
spring out of it, and the level around it stretches aside but stays visible, faintly, so you
always know where you are and what it connects to. Jumps across the hierarchy walk the tree
one level at a time.

## Running

```sh
cargo run                       # built-in example
cargo run -- path/to/model.c4   # a .c4 / .likec4 file
cargo run -- path/to/project    # every .c4 / .likec4 file under a directory
```

Files are watched and reloaded on save.

## Controls

| Input                        | Action                                         |
| ---------------------------- | ---------------------------------------------- |
| Click a card                 | Open it (or select it if it has nothing inside) |
| Click outside the frame, Esc | Go up a level                                  |
| Click a faint element        | Walk over to it                                |
| Scroll / pinch               | Zoom                                           |
| Drag, two-finger scroll      | Pan                                            |
| F                            | Fit to view                                    |
| R                            | Reload                                         |

## What it reads

A forgiving parser for the drawing-relevant parts of the LikeC4 DSL: `specification` element
kinds and their `style` (shape, color), `model` elements in both `name = kind` and
`kind name` forms, nesting, `extend`, `group`, and relationships (`->`, `-[kind]->`,
`.kind`, `<->`, implicit `this`). `views` and `deployment` blocks are skipped; rustyc4 lays
out each level itself.

Requires Rust 1.89 or newer.

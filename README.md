# rustyc4

A 3D viewer (and editor) for [LikeC4](https://likec4.dev) architecture models and
[Mermaid](https://mermaid.js.org) diagrams, built with [Bevy](https://bevyengine.org).

Every element is a block extruded from a neon grid floor, with its name painted on its top,
and relationships are glowing pipes routed between them at right angles; where two pipes
have to cross, one bridges over the other. There are no separate levels to switch between:
zoom in and any block that grows big enough rises into a platform, its title sliding to the
top edge as its children extrude out of it; zoom out and they sink away again. A pipe from
outside still runs into the container's side as before, and comes up through a small port
on the platform beside the component it belongs to, labelled with where it comes from or
goes to.

Four looks to choose from: **Tron** (neon edges on dark glass, with bloom), **Sketch**
(pen and ink on paper, with hatched sides and handwriting), **Blocky** (pixel-textured
voxels on grass, redstone pipes and chunky pixel text) and **Blueprint** (white linework on
cyanotype blue, blocks drawn like walls on a floor plan, technical capitals).

Dynamic views play as stories: pick one from the list and each step sends a pulse of light
along the pipe between its two blocks, in order, while the camera flies to each step and
everything not taking part dims.

## Running

```sh
cargo run                       # welcome screen: open a folder or file, or drop one in
cargo run -- path/to/project    # every .c4 / .likec4 file under a directory
cargo run -- path/to/model.c4   # a single file
cargo run -- --example          # the built-in example
cargo run -- --example mermaid  # the built-in Mermaid example
cargo run -- --style blueprint  # pick a look: tron (default), sketch, blocky or blueprint
```

Files are watched and reloaded on save.

## Editing

Press ⌘E (or click **Source**, top left) to open the source panel: a tree of the project's
files and folders, and an editor for the one you pick. The 3D view updates as you type.

- Problems are underlined with a squiggle, red for errors and amber for warnings, and
  explained in the status bar when the cursor is on them.
- Where there is a likely fix ("can't find `shop.gatway`; did you mean `shop.gateway`?"),
  ⌘. or the **Fix** button applies it.
- An autocomplete popup suggests keywords, element kinds and element names as you type;
  Tab or Enter accepts, ↑↓ choose, Esc dismisses.
- Brackets and quotes close themselves, Enter keeps (and after `{`, adds) indentation, and
  ⌘Z / ⌘⇧Z undo and redo. ⌘C / ⌘X / ⌘V use the system clipboard.
- ⌘S saves every changed file. Esc returns the keys to the 3D view.

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
| T                            | Switch style: Tron, Sketch, Blocky, Blueprint (also in the Esc menu) |
| Space, ← →, Backspace        | While a dynamic view plays: pause, step, stop  |
| Esc                          | Menu: open another project, reload, quit       |

## Mermaid

`.mmd` and `.mermaid` files are read alongside LikeC4 ones:

- **Flowcharts** (`flowchart` / `graph`) become part of the landscape: subgraphs are
  containers you can zoom into, nodes are blocks (`[(cylinders)]` are stores, `((circles))`
  people and `>flags]` queues; `<br>` splits a node's title from its description), and links
  (`-->`, `---`, `-.->`, `==>`, with `|labels|` or `-- labels -->`, chained, with `&`) are
  pipes. `classDef … fill:` and `style … fill:` colour them.
- **Sequence diagrams** become dynamic views you can play; participants are matched to
  flowchart nodes by name, and `par` blocks happen together.

## What it reads

A forgiving parser for the drawing-relevant parts of the LikeC4 DSL: `specification` element
kinds and their `style` (shape, color), `model` elements in both `name = kind` and
`kind name` forms, nesting, `extend`, `group`, and relationships (`->`, `-[kind]->`,
`.kind`, `<->`, implicit `this`), and `dynamic view`s (steps, `<-` replies, `parallel`
blocks). Other views and `deployment` blocks are skipped; rustyc4 lays out each level itself.

Requires Rust 1.89 or newer.

# CLAUDE.md

These are working rules for coding agents and contributors on xWorks, an
office suite built from scratch to be usable, fast and correct.

## Read first

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): design, decisions and tech
  stack. Settled decisions are marked with who settled them and when. To
  change one, raise it with the project owner first; don't work around it.
- [docs/TRACKS.md](docs/TRACKS.md): the tracks of work and their order.
- [docs/ROADMAP.md](docs/ROADMAP.md): milestones, their stages and exit
  criteria.

## Status and checks

M0 (architecture) is done. M1, the walking skeleton, is under way: stage 1
(the workspace and the test harness) is in place, and the other crates are
empty until their stages fill them ([docs/ROADMAP.md](docs/ROADMAP.md)).

Every change must pass, from the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo deny check
```

and the pure crates must build for WebAssembly (bash; CI runs the same):

```sh
pkgs=(); for d in crates/*/; do [ -f "$d/clippy.toml" ] || pkgs+=(-p "$(basename "$d")"); done
cargo build --target wasm32-unknown-unknown "${pkgs[@]}"
```

CI ([.github/workflows/ci.yml](.github/workflows/ci.yml)) runs Clippy and
the tests on Linux, Windows and macOS.

- `cargo run -p shot -- [SCENE...]` renders scenes to `target/shot/`;
  `--list` lists them. Look at what you render.
- Golden records and how they're approved: [spec/golden/README.md](spec/golden/README.md).

## Pure crates

Foundations, substrates and views are pure (docs/ARCHITECTURE.md, section
3): they do no I/O, open no threads, read no clocks or environment, and
print nothing. `crates/clippy.toml` forbids those calls, and Clippy applies
it to every crate under `crates/` that has no `clippy.toml` of its own. The
impure crates are `store`, `app`, `render-gpu` and `testkit`, each with its
own `clippy.toml`; `shells/` and `tools/` are impure too. Making another
crate impure needs the project owner's approval. A pure crate that needs
something from outside takes it as an argument.

## Engineering rules

1. **Tests first.** Acceptance tests, golden images or specs exist before
   the implementation. Don't weaken, skip or delete a test to make a change
   pass.
2. **Headless, with the same output every run.** Everything runs without a
   display or GPU, and produces identical output each time. Use `shot` to
   render results to PNG, and look at them.
3. **Plain, explicit Rust.**
   - Use the toolchain pinned in `rust-toolchain.toml`, edition 2024, with
     no nightly features.
   - No async runtime. The app runs work on a worker pool.
   - Prefer plain structs, enums and functions over macro or trait
     cleverness.
4. **`unsafe` is forbidden** workspace-wide. If a shell or renderer backend
   ever needs it, list the crate here first, with the owner's approval, and
   give every `unsafe` block a `// SAFETY:` comment. No crates are listed.
5. **Dependencies:**
   - Keep them few, well known and pinned, and prefer pure-Rust crates.
   - A new dependency needs a stated reason, in a comment beside it in the
     workspace's `Cargo.toml` and in the commit.
   - Licenses must be permissive; `deny.toml` lists them.
   - Read the pinned version's source and docs rather than recalling its
     API from memory.
6. **Small crates with narrow public APIs.** Each crate's `lib.rs` doc
   comment states its purpose and invariants.
7. **Specs are data.** Commands, functions, styles, limits and layout cases
   live in `spec/` as RON files, read by both code and tests. Never
   hard-code the same value in two places.
8. **Every reader of outside input gets a fuzz target:** files, fonts,
   Markdown and pasted data.
9. **Errors and logging:** `thiserror` in libraries, `anyhow` only in tools,
   `tracing` for logging, and serde with RON for data.
10. **Test data without a clear license** is fetched by CI, never committed.
11. **Golden images** are approved renders of our own output, recorded in
    `spec/golden/` as hashes of their pixels. Updating one needs a person's
    approval.

## Writing docs

- **Diagrams are Mermaid,** in fenced `mermaid` blocks, never box-drawing or
  ASCII art: text diagrams break when lines wrap. Directory listings in
  plain code blocks are fine.
- When the owner settles a decision, record it in the document it belongs
  to, attributed: *(project owner, YYYY-MM-DD)*.

## Commits

Commit as you go, in small steps that each make sense on their own, straight
to `master`. Don't push unless asked. A subject names the area and what
changed (`calc: decimal division rounds half to even`), and the body says
why when it isn't obvious.

## Decisions and reviews

The owner gives decisions in review sessions made with `rt` (review-tools),
not in prose or in the harness's question prompts. Run `rt` from inside this
clone, which connects it to the instance (rt.schjetne.dev, the project
gschjetne/xworks), with the review document outside the repository. Each
milestone is a work package (`m0` for the architecture, `m1`, …). Answers
become the repository's: record each settled decision in
docs/ARCHITECTURE.md or the plan it affects.

### Review sessions (review-tools)

When you stop for human review (a milestone is done, or you need decisions before going
on), do not ask for them in prose. Open a review session with `rt`: the human answers
numbered decisions in a browser and keeps talking to you here, in the harness, at the same
time.

1. Write a review document (Markdown) — what was built, what you learned, and the
   decisions you need, each as a fenced `decision` block under its own heading. Write it,
   and any screenshots it links to, **outside the repository** (a temporary directory
   will do): the review session stores the document, its attachments and the final record,
   so none of it belongs in git.

   ````markdown
   ---
   work_package: m1
   title: "Milestone 1: offline sync"
   preview: http://localhost:3000
   ---

   ## Context
   What was built and what was learned. Link screenshots and notes with relative paths;
   they are uploaded with the document: ![conflict](shots/conflict.png)

   ## Decision: Conflict resolution strategy
   ```decision
   id: conflict-strategy
   kind: choice
   options:
     - last-writer-wins (simplest, loses edits)
     - per-field merge (what the prototype does)
   recommend: per-field merge
   ```
   Why it matters, trade-offs, evidence.

   ## Decision: Ship milestone 1?
   ```decision
   id: ship-m1
   kind: go-nogo
   recommend: go
   ```
   ````

   Kinds: `go-nogo`, `choice` (add `multi: true` for several answers), `text`. The human
   can add free text to every decision. Ids are stable: keep them when you revise.
   Numbers are assigned by the session. See `docs/agent/writing-review-documents.md` in
   the review-tools repository for the full format and advice.

2. `rt review open review.md` — prints the review page URL. Give it to the human.

3. The loop:
   - `rt review wait` blocks until the human **sends** answers from the page, prints what
     changed, then exits. Run it in the background if your harness can, so you keep
     talking with the human meanwhile. It waits 9 minutes by default (`--timeout`); if
     your shell tool has a shorter limit, pass a shorter timeout and call it again. Exit
     status 3 means this agent session was detached or the review closed: stop.
   - **Harness first.** If answers arrive while you are working on something the human
     asked in the harness, finish that and reply to it before you turn to the review.
   - `rt review state` shows everything, including the human's unsent drafts and answers
     that went stale when you revised a decision.
   - Tell the human, in the harness, what you made of their answers. The review page has
     no chat; its log keeps events and your notes. Post a one-line note of anything
     settled in conversation: `rt review post "…"` (`--about 3` ties it to decision 3).
   - When answers change the questions, edit the document and run
     `rt review update review.md`. Lost your copy, or taking over from another agent
     session? `rt review document --out <file>` writes the current one; the files it
     links to are already stored, so they need not be next to it. Decisions are added,
     revised (the human confirms earlier answers to them) and withdrawn by id. Withdraw explicitly with
     `withdrawn: <reason>` in the block, or by removing it. Say why, in the harness and in
     a note.
   - When the human answers in conversation ("go with redb"), record it against the
     decision so the form reflects it: `rt review record 4 --choice redb`. They confirm
     it on the page.
   - Then `rt review wait` again.

4. When every decision has a sent answer and you have nothing more to add, run
   `rt review ready` (`--note` says in a line what the human is accepting). The human then
   **accepts** the review with the button next to Send; don't ask them to accept with a
   decision. Acceptance wakes your `rt review wait`. Do what it means for the work, then
   `rt review close` prints the Markdown record, which stays with the session (the
   review page links to it); do not commit it. A Send or a
   revision clears "ready", so mark it again when you are done with those.

If your session ends before the review does, the human can hand the review to a fresh
agent session. All it needs is `rt review attach <id>`, which prints where the review
stands and how to work it.

#### On a review-tools instance in the cloud

On a person's machine, they connect a clone once with `rt init`, which opens their
browser; after that `rt` in that clone uses the instance. Elsewhere,
`RT_ENDPOINT` points `rt` at the instance, and the token comes from `RT_TOKEN`, or in a
Claude Code cloud environment from a network secret that the environment's proxy adds to
your requests; you never see it. `rt whoami` shows which server you reach and as whom. If `rt`
is missing, install the client that matches the instance:

```sh
curl -fsSL "$RT_ENDPOINT/downloads/install.sh" | sh
```

If `rt` says a token is missing or refused, ask the human to add or renew the network
secret (or `RT_TOKEN`); do not ask them to paste a token into the conversation.

Everyone with read access to the repository can read its reviews on the instance, every
revision of the document included. Never put credentials or other secrets in a review
document or a note; the server refuses text that looks like one.

In a Claude Code cloud session nothing can wake you from outside, and the session pauses
when idle. So keep `rt review wait --timeout 25m` running in the background and start it
again each time it returns, while you wait for answers. When the human writes to you after
a pause, run `rt review state` before anything else: answers may be waiting for you.

Every command that reads takes `--json`. `rt <command> --help` documents each one.

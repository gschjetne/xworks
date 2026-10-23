# Larkspur

[![Build](https://ci.example.org/larkspur/badge.svg)](https://ci.example.org/larkspur)
[![Licence: MIT](https://img.example.org/licence-MIT-blue.svg)](LICENSE)

Larkspur is a small command-line tool that keeps a garden's planting
calendar in plain text. It reads `garden.toml`, works out what to sow
and when, and prints a week-by-week plan.

> It's the tool I wanted every March, when the seed packets come out of
> the drawer and none of them agree about anything.

## Installing

You need Rust 1.80 or later. Then:

```sh
cargo install larkspur
larkspur --version
```

Or download a release from the [releases page][releases].

## Using it

1. Write a `garden.toml` describing your beds.
2. Run `larkspur plan` in the same folder.
3. Pin the printout somewhere you'll see it.

The plan has three columns:

| Week | Sow indoors | Sow outside |
|------|-------------|-------------|
| 10   | Tomatoes    |             |
| 12   | Peppers     | Broad beans |
| 15   |             | Carrots     |

Options:

- `--from DATE` starts the plan on another day.
- `--frost DATE` sets the last frost date, which most crops count from.
  Without it, Larkspur guesses from your latitude.
- `--json` prints the plan as JSON, for other tools.

### Configuration

Each bed is a table:

```toml
[beds.north]
size = "1.2 x 3"
crops = ["carrots", "beetroot"]
```

**Note:** sizes are in metres. *Imperial units* aren't supported yet;
see [issue 14](https://example.org/larkspur/issues/14).

## Contributing

Bug reports and patches are welcome. Please run `cargo test` before
sending a patch, and keep commits small.

## Licence

MIT, see [LICENSE](LICENSE).

[releases]: https://example.org/larkspur/releases

# ADR 0104 — Decision reports and local times in `jk decisions`

**Status:** accepted (v1) · **Date:** 2026-11

## Context

Document 07 §4 lists `jk decisions [--output <s>] [--since 1h | --from … --to …] [--cause <signal>] [--report pdf|csv]`, and says the compliance report is made by `jk`, not by the node. ref/09 §5.7 fixes the CSV columns, ref/09 §9.1 says `jk` asks `GET /v1/decisions` for `text/csv` and makes the PDF locally, and ref/09 §3.7 says the local times of `jk` (`--since "2026-09-24 03:00"`) are converted by `jk` with the client's time zone. Four things are left open (AUR-268):

- where the report goes: the command has no file option, and a PDF is binary;
- how much of the log a report covers, since `GET /v1/decisions` is paginated;
- what the PDF contains and how it is built;
- which local forms `--since`, `--from` and `--to` accept. The incident example of document 07 §6 passes an instant to `--since` and a bare time to `--to` (`--since "2026-09-24 03:00" --to "03:30"`).

## Decision

1. **Times.** `--since` takes a duration (`1h`) or an instant; with an instant it is `--from`. `--since` with `--from` is a usage error (exit 1). An instant is RFC 3339 (sent as is), `now`, `-<duration>`, a local date and time `YYYY-MM-DD HH:MM[:SS]`, or a local time of day `HH:MM[:SS]`. A time of day takes the date of the other bound when it has one (`--to "03:30"` after `--from "2026-09-24 03:00"`), and today's otherwise. Local forms are converted with the client's time zone (`TZ`, else the system's) into UTC before the request; a local time that does not exist or is ambiguous at a daylight-saving change takes the earlier instant.
2. **The report covers the whole selection.** With `--report`, `jk` asks with `order=asc` and `limit=1000` and follows `next_cursor` until the end; `--limit` does not apply.
3. **Where it goes.** `--out <file>` writes the report to a file and never overwrites one that exists (exit 1). Without `--out`, `--report csv` writes to standard output, and `--report pdf` is a usage error (exit 1).
4. **CSV.** The report is the node's `text/csv` (ref/09 §5.7), its pages joined with one header line.
5. **PDF.** `jk` builds it from that CSV, with no other input, through the `pdf-writer` crate (pure Rust, no system libraries). A4 landscape, the standard Courier and Courier-Bold fonts, no embedded fonts or images. The first lines of every page carry the title (`Decision log`), the node, the filters as sent (outputs, cause, range in UTC), the number of entries and the instant the report was made (UTC); then the CSV header and one line per entry, with the columns of ref/09 §5.7 in that order and of fixed width. A cell longer than its column is cut and ends in `~`; characters outside printable ASCII are written as `?`. The CSV is the complete record; the PDF is its printable view. The footer of each page is `page <n> of <m>`. The document carries no creation date, so the same CSV, filters and instant give the same bytes.

## Consequences

- `jk decisions` gains `--from`, `--to`, `--report` and `--out` (document 07 §4, ref/09 §9.1).
- `GET /v1/decisions` must apply `from`, `to` and `since` and serve `text/csv` and `application/x-ndjson`, as ref/09 §3.5, §3.7 and §5.7 already say; the node did neither before AUR-268.
- `jk` depends on `pdf-writer` and on `jiff` (time zones). Neither reaches the node.
- A report over a long range reads the whole range from the node, one page at a time; the range bounds the work.

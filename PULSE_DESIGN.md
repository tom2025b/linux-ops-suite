# Pulse Design

Pulse is a read-only, minimal TUI status instrument for the Linux Ops Suite.
It should open to a calm verdict, not a dense dashboard.

## Core Direction

Pulse answers one question first:

> Is the suite healthy right now?

The default screen should feel like a high-end monitoring instrument at rest:
quiet, spacious, and precise. When everything is fine it should look almost
empty — closer to a watch face than a control panel.

Pulse is not RexOps with another skin. RexOps remains the launcher and
orchestrator. Pulse is the read-only overview: the place to understand whether
anything deserves attention before deciding where to work. RexOps now *opens*
into Pulse (a bare `rexops` shows the verdict first), and Pulse offers a single
way back up into the cockpit — the `r` key launches `rexops tui`. Pulse hands
off to the launcher; it never becomes one.

The guiding instinct for every layout decision: **when the suite is healthy,
remove things.** Detail, counts, source markers, and key hints all earn their
place only when they explain a problem. A healthy Pulse is mostly negative
space.

## Research Basis

This design is grounded in the current Linux Ops Suite architecture and contract
hub.

### Repository Role

`linux-ops-suite` is the contract and index headquarters for the suite. It holds
the shared architecture docs, integration map, contract rules, JSON schemas,
example fixtures, and shared TUI chrome.

It is not a monorepo for all tools. Most tool logic lives in sibling repos. That
matters for Pulse: it should consume published artifacts and file contracts, not
reach into another tool's internals.

### Data-Flow Model

The suite is designed around one-way, file-based contracts:

```text
Bulwark ----------- workstate-feed JSON ----> Workstate
ToolFoundry ------- workstate-feed JSON ----> Workstate
Proto ------------- workstate-feed JSON ----> Workstate
Workstate --------- snapshot JSON ----------> suite consumers
Toolbox-Bridge ---- sidecar feed -----------> ScriptVault
RexOps ------------ snapshot/report --------> self/report
```

The important design implication: Pulse should be a passive reader. It should
never mutate producers, regenerate feeds, or try to repair state from the
dashboard.

### Tool Responsibilities

Current suite responsibilities from the architecture docs:

- Bulwark owns read-only inventory and risk/language classification.
- ScriptVault owns human-facing script search, preview, favorites, recents, and
  launch.
- Toolbox-Bridge converts Workstate findings into ScriptVault sidecar metadata.
- ToolFoundry owns tool lifecycle, ownership, health, drift, and manifests.
- Workstate owns read-only project/repository health.
- RexOps is the suite-level consumer/orchestrator and launcher.

Pulse should sit beside RexOps conceptually as a read-only instrument. It should
not become the launcher — but, as RexOps' default screen, it provides one
hand-off *to* the launcher (`r` → `rexops tui`). Launching the cockpit is a
foreground process hand-off, not Pulse taking on launcher responsibilities: it
spawns one known sibling binary and resumes when that exits. Pulse itself stays
read-only and owns no tool registry, job manager, or catalog.

### Existing Producer Contracts

The integration map currently identifies these key producer/consumer surfaces:

- Workstate snapshot -> Toolbox-Bridge.
- Toolbox-Bridge `workstate-feed` -> ScriptVault.
- ToolFoundry `workstate-feed` -> Workstate.
- Bulwark `workstate-feed` -> Workstate.
- Proto session records and `workstate-feed`.
- Bulwark scan, ScriptVault export, Workstate snapshot, and RexOps snapshot as
  suite-level reporting inputs, with some still provisional.

Current examples show the kind of information Pulse can summarize:

- Source freshness: `Fresh`, `Stale`, unsupported version, missing data.
- Provenance: feed id, fetched time, source-observed time, dropped records.
- Tool lifecycle and drift: attention count, drifted tools, health passed/total.
- Attention items: tool, id, reason, severity.

### Freshness And Confidence

Pulse treats freshness as a confidence signal, not as a table on the opening
screen.

The default source state exists because the suite can be partially present:
producers are optional, files can be missing, versions can be unsupported, and
some feeds can be stale while the rest of the suite is usable.

That is why the design separates three layers, surfaced progressively:

- Verdict: what the operator should know first.
- Cause rows: why the verdict changed (only when it changed).
- Sources: whether the verdict is trustworthy (only when trust is in question).

When the suite is healthy and current, layers two and three are silent. Their
absence is the signal: nothing is asking for attention.

### Shared UI Language

The suite already has shared TUI chrome in `suite-ui` and `thomas-tui`:

- `Theme` with cyan/amber accents and `NO_COLOR` handling.
- Common health styling: healthy, degraded, unavailable, unknown.
- Common severity styling: critical, high, medium, low.
- Shared panes, overlays, help sheets, command-palette framing, search bars,
  key hints, freshness stamps, status strips, and health strips.

Pulse should reuse that visual vocabulary, but be far more restrained than the
existing launcher-style TUIs. It uses the shared language without showing the
shared widgets on the first screen. It borrows the theme, not the chrome.

### UX Constraints From Prior TUI Work

The suite has already surfaced a few useful TUI constraints:

- Real terminal testing matters; static layout review misses clipping and
  awkward interaction.
- Compact terminals around 80x24 need special care.
- Confirmation and close flows should not rely on Escape alone, because the user
  may be on an iPad-style keyboard.
- Read-only status screens should avoid implying that unavailable entries are
  interactive.
- RexOps launcher behavior and foreground child-process handoff are separate
  concerns; Pulse should avoid that whole class of complexity by staying
  read-only.

These constraints support the restrained default design: fewer visible controls,
fewer panes, and no mutating action path.

## The Default Screen

The opening screen has one job: render the verdict in a calm, centered field of
space. Everything else is conditional.

The healthy screen is the design's true north. It should be the emptiest screen
in the entire suite.

### Healthy (the default of the default)

```text








                                  all clear




                                                                  2m ago
```

That is the entire screen. One verdict, alone, just above center. One faint
relative time, alone, in a far corner. Nothing else — no wordmark line, no
supporting sentence, no source markers, no cause rows, no rule, no key hints.

The screen is two pieces of text on an open field. Read top to bottom it is a
single breath: *all clear … 2m ago.*

Notes on this screen:

- The verdict reads `all clear`, lowercase. A calm state does not shout;
  all-caps is reserved for states that need urgency. It is the only prominent
  text on screen, so it is unambiguously the focal point.
- The supporting line is gone. `suite healthy · current` said nothing the
  verdict and the timestamp did not already imply. "Healthy" *is* "all clear";
  "current" *is* the presence of a fresh timestamp. Saying it twice is noise.
- The wordmark is gone from the healthy screen. Identity does not need to repeat
  on a screen the operator opened on purpose; `?` and the title bar still carry
  it. (It returns on the busier states as a quiet top-left anchor.)
- The timestamp is reduced to the relative value, `2m ago` — no `updated`
  label. It is the dimmest mark on screen and the only thing in the lower half.
  It exists so a glance confirms the data is live. If even that is too much, a
  config option may hide it; the verdict alone is a valid healthy screen.
- All keys still work; no hint strip is drawn. `?` reveals the strip and help.
  See Navigation.

This is the meditative state: an almost-empty terminal whose silence is the
message. Two soft words and a number, surrounded by space, saying *you can look
away.*

### Needs Attention

When the verdict changes, the screen earns its detail. Layout position is
identical; the space simply fills from the center outward.

```text
 pulse




                                NEEDS ATTENTION

                              2 critical · 4 high


                       confidence reduced by stale feeds


             deploy-prod.sh      token-like secret           bulwark
             findings            unsupported feed version    workstate


             sources   ● workstate  ● bulwark  ◐ toolfoundry  ○ vault


 ────────────────────────────────────────────────────────────────────────────
 enter  details      a  attention      f  feeds      /  search      r  cockpit      ?  help
```

What changed from healthy, and why:

- Verdict goes all-caps: `NEEDS ATTENTION`. Urgency is allowed here.
- Counts collapse onto one line: `2 critical · 4 high`. Two stacked numbers read
  as a tally; one line reads as a sentence and stays calmer.
- The wordmark returns top-left, dim. On a busy screen a quiet anchor helps;
  on the silent healthy screen it was clutter.
- The confidence line appears only if confidence is actually reduced. If feeds
  are fresh, this line is omitted and the space stays open.
- Two cause rows appear (three on tall terminals). Never a scrollable table.
- The source line appears, because once there is a problem the operator needs to
  know whether to trust it.
- The hint strip appears, because the operator is now likely to act.
- The timestamp regains its `updated` label here; with other text on screen the
  bare `2m` would be ambiguous. The label is dropped only on the healthy screen,
  where nothing else competes with it.

### Incomplete

```text
 pulse




                                  INCOMPLETE

                             2 sources unavailable


                        the suite view may be missing data


             sources   ● workstate  ● bulwark  ○ toolfoundry  ○ vault


 ────────────────────────────────────────────────────────────────────────────
 enter  details      a  attention      f  feeds      /  search      r  cockpit      ?  help
```

Incomplete is distinct from Needs Attention: the suite is not unhealthy, it is
*unsure*. The source line is the focal evidence here, so it is always shown.
No cause rows — there are no findings to explain, only absences.

### Layout invariants

The vertical structure is fixed across all three states. Elements appear and
disappear, but they never move position. This stability is the premium feel.

```text
 [wordmark]                                               row 1, top-left, optional

 [ ── open space ── ]                                     vertical breathing room

 [VERDICT]                                                anchored ~40% height
 [supporting count / line]                                centered, optional
 [confidence line]                                        centered, optional

 [cause row 1]                                            optional block
 [cause row 2]

 [sources line]                                           optional, centered

 [ ── open space ── ]
 [rule + hint strip]                                      bottom, optional
 [timestamp]                                              corner, always (dim)
```

The verdict anchor sits slightly above true center (golden-ratio high), so the
healthy screen's empty lower half reads as intentional calm rather than missing
content. On the healthy screen every optional row is absent, leaving only the
verdict at the anchor and the timestamp in the corner.

## Cause Rows

The default screen shows only the smallest useful explanation for the verdict,
and only when there is a verdict to explain.

```text
             deploy-prod.sh      token-like secret           bulwark
             findings            unsupported feed version    workstate
```

Rules:

- Healthy state shows zero cause rows.
- Non-healthy states show two rows by default, three only on taller terminals.
- Never a scrollable table on the default screen. Everything beyond the top two
  belongs in the Attention view.

Each row answers three things, left to right, in three soft columns:

- What is affected?
- Why does it matter?
- Which source reported it?

Indentation, not borders, separates the columns. No `│`, no `─`, no grid.

## Source Confidence Line

The source line is not a health table. It is a quiet trust indicator, and it is
**hidden whenever every source is current.** Showing a row of green dots on a
healthy screen is operational noise; a healthy Pulse simply omits the line, and
that silence reads as full confidence.

It appears the moment any source is stale or missing:

```text
             sources   ● workstate  ● bulwark  ◐ toolfoundry  ○ vault
```

Marker mapping:

- `●` current — filled; green when color is enabled.
- `◐` stale / degraded — half-filled; amber when color is enabled.
- `○` missing / unavailable — hollow; dim gray.

The label `sources` is lowercase and dim, set apart by spacing. Current sources
are listed plainly; the eye is drawn only to the half and hollow markers, which
are the ones that matter. ASCII fallbacks (`*`, `~`, `o`) remain readable under
`NO_COLOR`.

## Color Rules

Color is rare and always carries state. A healthy screen is almost monochrome.

- Green: the healthy verdict and current sources.
- Amber: needs-attention verdict, stale data, degraded confidence.
- Red: critical counts, and unavailable sources only when they affect the
  verdict.
- Cyan: focus or selection only, never decoration.
- Dim gray: wordmark, labels, timestamps, optional missing data, secondary text.

The interface must remain fully legible with color disabled. State is always
also carried by word and by marker shape, never by color alone.

## Navigation

The default screen is glance mode. It becomes a tool only when the user asks.

On the healthy screen the hint strip is hidden to protect the calm. It fades in
on the first keypress, and is always present on the Needs Attention and
Incomplete screens, where action is likely.

```text
 enter  details      a  attention      f  feeds      /  search      r  cockpit      ?  help
```

Primary paths:

- `Enter`: open details for the current verdict or selected cause.
- `a`: open the full Attention view.
- `f`: open feed freshness and source confidence.
- `/`: search across visible suite status data.
- `r`: open the full RexOps cockpit (`rexops tui`) — the way out from the calm
  status screen to the launcher/jobs interface. Pulse is RexOps' default screen
  (a bare `rexops` opens Pulse), so `r` is the deliberate step *up* into the
  cockpit when the operator decides to act. It foreground-launches RexOps,
  handing it the real terminal, and returns to Pulse when the cockpit exits. If
  `rexops` isn't on PATH this is a no-op with a single dim status line, never an
  error — the same graceful degradation the rest of the suite follows. `r` is a
  literal character inside the search box, not the shortcut.
- `?`: help, and reveal the hint strip if hidden.
- `q`: quit (always works; intentionally omitted from the strip to reduce its
  width and visual weight).

Avoid Escape-only flows. Any modal or drill-down has an obvious non-Esc close
path.

## Design Principles

- One verdict owns the screen, and on a healthy suite it owns the screen alone.
- Negative space is the primary material; the healthy screen is almost empty.
- Cut any element the verdict already implies. "Healthy" need not also say
  "current," "no findings," or "all sources up" — silence carries those.
- The interface reveals detail progressively — wordmark, counts, causes, sources,
  and hints each appear only when they explain something.
- A calm state never shouts: lowercase verdict, no caps, no markers, no strip,
  no label on the timestamp.
- Counts appear only when they explain the verdict, and read as one line.
- Source freshness signals confidence, and is silent when confidence is full.
- Elements may appear or vanish, but never move. Position is constant.
- Pulse opens as an instrument at rest, not an information dump.

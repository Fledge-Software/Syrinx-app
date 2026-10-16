# Your Syrinx usage report

This page is for you, the owner. It explains what Syrinx's usage report
contains, what it never contains, and what happens when you share one. The
rules behind it are decision D39 in `docs/decisions.md` and the approved
proposal, `docs/usage-report-privacy.md`.

## What it is for

When counting is on, Syrinx keeps a set of numbers about how its own
features are used, so that you can later hand them to a model of your choice
and ask what to improve. Nothing is sent anywhere by Syrinx, at any time.
Counting is off until you switch it on in Customize ("Count how I use
Syrinx, on this computer only"), and switching it off deletes every count.

## How to see and share it

1. Open Customize and find **Usage report**. With counting on, press
   **View report**.
2. The report view shows the **current period** (from the day counting
   started, or the last Reset, until today) or, once a period has run its
   30 days, the **previous period**. Syrinx keeps at most these two.
3. The view shows the report as a table. Switch on **Show exact file** to
   see the precise text that would leave your computer. The table is read
   from that same text, so the two always agree.
4. **Save report…** opens Windows' own save dialog with the name
   `syrinx-usage-report-<period_start>-to-<period_end>.json`. You choose the
   folder; Syrinx does not remember it or add the file to Windows' recent
   items. (Windows' dialog itself may open in the folder you used last; that
   is Windows' memory, not Syrinx's.)
5. **Copy report** puts the same text on the clipboard, for pasting into a
   model chat. If Windows clipboard history or "Sync across your devices" is
   on, Windows keeps the copied report in that history and may sync it to
   your other devices; Save report… avoids this.
6. **Copy prompt** copies only the suggested prompt shown below the buttons,
   never any of the report.

Save report… and Copy report are the only ways a report leaves Syrinx.
Syrinx never uploads it, and both are unavailable while counting is off.

## What a report contains

A report is a small JSON file with exactly these parts, every field present
even when it is zero:

- **The envelope**: the schema version (1), Syrinx's own version number
  (the same for every copy of that build), the first and last day of the
  period (day precision only), the number of days in the period on which
  Syrinx started, and a fixed paragraph explaining the units.
- **Feature counts**: how many times each Syrinx feature was used, such as
  tabs opened, the palette opened with Ctrl K, or a saved page opened.
- **Command counts**: how many times each built-in palette command ran.
  Any other command, such as a plugin's, is counted only as
  `command.other`, never by name.
- **Timings**: for eight measured operations (start-up, palette, search,
  tab switches, saves, backups), how many fell into each fixed time range.
  Only the count per range is kept, never a single measurement or when it
  happened.
- **Error counts**: how many times each fixed kind of failure happened,
  such as a page's renderer stopping. Never an error message or code.

## What a report never contains

- No web addresses, page titles, searches, typed text, note or code text,
  or file paths.
- No names of plugins or of commands you defined.
- No account, device, installation or session identifier, and no hash of
  one.
- No timeline: no time of any event and no per-day breakdown, only totals
  for the period and one count of days used.
- No counts of sensitive kinds of use, such as how often banking sites
  were skipped.
- No metadata added by Syrinx: no path, user name or time beyond the two
  period dates. (Windows records when a saved file was created, as for any
  file.)

The exact format is UTF-8 text without a byte-order mark, with LF line
endings, two-space indentation and a final newline, and its keys always
appear in the same order.

## A one-person report is not anonymous

Removing identifiers does not make a report anonymous when it describes one
person. Whoever receives your report can tell it came from you, and can see
roughly how much you used Syrinx during the period. Share it only with a
service you would trust with that, and read the exact file first if you
want to be sure of what it says.

## The suggested prompt

The report view shows this prompt, which you can copy and paste with the
report:

> This is a usage report from Syrinx, a desktop browser I use. It contains
> only aggregate counts and timing buckets for one period, described in its
> "about" field. Suggest up to five changes that might make Syrinx faster or
> easier for me, each tied to the numbers that support it. Say what each
> number cannot tell you, and do not assume why I did something from how
> often I did it.

You review whatever the model suggests and decide what, if anything,
becomes work on Syrinx.

# Plugin runtime bounds: deadlines and the queue

Plan 2's P2-03 asks that the workload and the queue and timeout bounds be
fixed before the runtime's behaviour is judged. This page is that record for
#135: the synthetic workload, what it measured, and the deadline of each
export and the size of each plugin's queue that follow from it. The figures
live in code in `crates/syrinx-plugin-host/src/deadline.rs`
(`Export::deadline_ticks`) and `src/limits.rs` (`EPOCH_TICK`,
`PLUGIN_QUEUE_CAPACITY`); change them here and there together.

## Terms

- **Export**: a function a plugin provides to Syrinx in the `syrinx:plugin`
  world (`wit/v0_1/plugin.wit`), such as `commands.run`.
- **Epoch deadline**: wasmtime's interruption mechanism. One ticker thread
  per engine advances a counter every `EPOCH_TICK` (10 ms), and compiled
  plugin code traps with `Trap::Interrupt` once the counter passes the
  store's deadline. Syrinx gives each export call a fresh deadline of a
  whole number of ticks. The ticker runs only while at least one call is in
  flight and waits on a condition variable otherwise, so an idle Syrinx
  never wakes for it (D6's idle CPU); the lifecycle test
  `the_epoch_does_not_tick_while_every_plugin_is_idle` checks that.
- **Worker queue**: each active plugin has one worker thread that owns its
  store and runs calls one at a time from a bounded queue. A call that
  finds the queue full is answered `unavailable` at once, without waiting.

## The synthetic workload

The workload is the full test fixture
(`crates/syrinx-plugin-host/tests/fixtures/plugins/full`) under the test
host, with synthetic in-memory services. It is the test
`lifecycle::workload` in `crates/syrinx-plugin-host/tests/contract.rs`;
run it with

```
cargo test -p syrinx-plugin-host --test contract -- workload --nocapture --test-threads=1
```

It times each export as the host makes it, including the deferred calls
the export queued:

| Export | What one sample is | Samples |
|---|---|---|
| `lifecycle.activate` | activating a freshly instantiated plugin | 20 instances |
| `lifecycle.deactivate` | deactivating an activated one | 20 instances |
| `commands.run` | `test.echo`: settings write and read, a feed signal, a log line | 200 |
| `forms.form-for` | the form with every field kind | 200 |
| `forms.validate` | one valid field set | 200 |
| `forms.submit` | a submit that finishes at once | 200 |
| `views.opened` | `test.open`: two views opened, one refused, metadata and an event posted | 200 |
| `views.message` | one queued view message delivered | 200 |
| `views.closed` | closing an open view | 200 |
| `search-feed.changes` | a four-change batch with over-limit keywords dropped | 200 |
| `search-feed.resolve` | one address | 200 |
| `events.deliver` | completing a `pending` submit | 200 |

The queue test, `a_full_queue_of_workload_calls_drains_quickly`, fills one
plugin's queue with the workload's most frequent call, a form validation
(the palette validates as the owner types), and times how long the worker
takes to answer them all.

## What it measured

Linux, AMD Ryzen 5 5600X (12 threads), the `dev` test profile, 2026-11-12.
Each figure is milliseconds.

| Export | p99 | Slowest |
|---|---|---|
| `lifecycle.activate` | 0.129 | 0.129 |
| `lifecycle.deactivate` | 0.019 | 0.019 |
| `commands.run` | 0.060 | 0.087 |
| `forms.form-for` | 0.052 | 0.074 |
| `forms.validate` | 0.033 | 0.040 |
| `forms.submit` | 0.035 | 0.049 |
| `views.opened` | 0.114 | 0.153 |
| `views.message` | 0.043 | 0.046 |
| `views.closed` | 0.025 | 0.026 |
| `search-feed.changes` | 0.058 | 0.080 |
| `search-feed.resolve` | 0.026 | 0.031 |
| `events.deliver` | 0.027 | 0.037 |

A full queue of 64 validations drained in 1.9 ms.

These are not the owner's Windows machine. #28's Windows measurements
covered start-up, memory and compile time, not per-call time; Windows CI
runs the same tests on every pull request, and their assertions (below)
hold there or the gate fails.

## The deadlines

A deadline is not an expected duration. It is the ceiling past which a
plugin is treated as stuck: its store is discarded, every write it had in
flight is reported `uncertain`, and it is shown as failed until its next
explicit use. So each deadline is set by how long the owner can wait on that
kind of call, then checked against the workload: every export's slowest
workload call must stay at least ten times inside its deadline
(`HEADROOM` in the workload test), and in the measurement above every one
stays more than six hundred times inside it.

| Export | Ticks | Deadline | Why this ceiling |
|---|---|---|---|
| `lifecycle.activate` (and instantiation) | 100 | 1 s | Runs once, on first use; a plugin may read its settings and open views. |
| `commands.run`, `forms.submit` | 100 | 1 s | An owner-initiated action that may do real work through host services. |
| `search-feed.changes` | 100 | 1 s | Up to 512 changes per batch (`MAX_BATCH_CHANGES`); runs in the background. |
| `views.opened`, `views.message` | 50 | 500 ms | A plugin tab waits on these; half a second is the longest a tab should look frozen. |
| `forms.form-for` | 25 | 250 ms | Drawn in the palette's preview pane as the owner moves through results. |
| `views.closed`, `search-feed.resolve`, `events.deliver`, `lifecycle.deactivate` | 25 | 250 ms | Bookkeeping; deactivation is best effort at shutdown, so it must not hold Syrinx's exit. |
| `forms.validate` | 10 | 100 ms | Runs as the owner types; a slow validator must not hold the next keystroke. |

Rejected: one deadline for every export (the 1 s that #28 used). It let a
validator stall typing for a second, and gave deactivation as long as an
action, which would hold shutdown.

## Host services

An epoch deadline cannot interrupt a host function the plugin is waiting
on. So before every export the host hands its services the call's deadline
(`PluginHost::begin_export`), and a service that waits on something gives
up after `CallDeadline::service_budget`, half the time left, answering
`unavailable`. Half, so the plugin still has time to handle that answer
before its own deadline traps it. `host.cancel` reaches the service too
(`host::Host::cancel`), after the host has dropped anything queued for that
request. The test `an_unavailable_host_service_answers_by_the_deadline_and_the_plugin_keeps_running`
uses a service whose dependency never answers; without the deadline it
would wait for ever.

## The queue

`PLUGIN_QUEUE_CAPACITY` is 64 calls per plugin. The workload shows a healthy
plugin answers a full queue in about 2 ms, so a queue only fills when a
plugin is slow or stuck. The size then decides two things:

- **How much work can pile up behind a stuck plugin.** Not how long a
  caller waits: when the stuck call reaches its deadline the plugin is
  discarded and every queued call is answered `unavailable` at once, so the
  longest wait is one deadline (1 s) whatever the queue's size.
- **The largest legitimate burst it absorbs.** The workload's largest
  burst is a palette form being typed into (one validation per keystroke,
  about 30 for a long field) while a plugin tab sends its start-up
  messages and the search slice pulls a feed: about 32 calls. 64 is twice
  that.

Rejected: an unbounded queue, which lets a stuck plugin collect calls
without limit; and a queue of one, which would refuse a keystroke's
validation while the previous one is still running.

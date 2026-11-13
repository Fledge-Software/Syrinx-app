# Plugin runtime bounds: deadlines and the queue

Plan 2's P2-03 asks that the workload and the queue and timeout bounds be
fixed before the runtime's behaviour is judged. This page is that record for
#135: the synthetic workload, what it measured, and the deadline of each
export and the size of each plugin's queue that follow from it. The figures
live in code in `crates/syrinx-plugin-host/src/deadline.rs`
(`Export::deadline_ticks`) and `src/limits.rs` (`EPOCH_TICK`,
`PLUGIN_QUEUE_CAPACITY`, `JOB_BUDGET`); change them here and there together.
The queue size and deadlines come from this workload, not from #28, whose
Windows measurements covered start-up, memory and compile time.

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
host, with synthetic in-memory services. It is
`crates/syrinx-plugin-host/tests/lifecycle/workload.rs`, part of the
`contract` test binary; run it with

```
cargo test -p syrinx-plugin-host --test contract -- workload --nocapture --test-threads=1
```

It times each export as the host makes it, including the deferred calls
the export queued:

| Export | What one sample is | Samples |
|---|---|---|
| `lifecycle.activate` | instantiating a prepared component and activating it, which share one deadline | 20 instances |
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
| `lifecycle.activate` (with instantiation) | 0.576 | 0.576 |
| `lifecycle.deactivate` | 0.020 | 0.020 |
| `commands.run` | 0.073 | 0.099 |
| `forms.form-for` | 0.054 | 0.073 |
| `forms.validate` | 0.033 | 0.042 |
| `forms.submit` | 0.036 | 0.054 |
| `views.opened` | 0.118 | 0.152 |
| `views.message` | 0.043 | 0.044 |
| `views.closed` | 0.028 | 0.029 |
| `search-feed.changes` | 0.058 | 0.081 |
| `search-feed.resolve` | 0.026 | 0.041 |
| `events.deliver` | 0.029 | 0.040 |

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
kind of call, then checked against the workload: every export's 99th
percentile must stay at least ten times inside its deadline (`HEADROOM` in
the workload test), and its slowest call inside the deadline itself, since
one call preempted on a loaded machine can lose 10 ms or more of wall
time. In the measurement above the slowest call of each export, the
stricter figure, still stays far inside: the closest is
activation, at about 1,700 times inside its deadline; `forms.validate`, with
the shortest deadline, is about 2,400 times inside it.

| Export | Ticks | Deadline | Why this ceiling |
|---|---|---|---|
| `lifecycle.activate`, with instantiation | 100 | 1 s | Runs once, on first use; a plugin may read its settings and open views. Instantiation (the component's start functions) and `lifecycle.activate` share this one deadline, and a trap in either is a trap in activation. |
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

The application's settings service (#136, `owner/services.rs`) follows the
same rule. Syrinx's store can wait up to SQLite's 5 s busy timeout, so a
plugin's settings reads and writes go to one records thread of their own
(`owner/records.rs`), and the service waits for the answer only its budget.
Past that, or when 16 pieces of work are already queued, it answers
`unavailable`. Work already queued still runs, so a write answered
`unavailable` may still land; settings writes overwrite, so a retry is
harmless. `a_busy_store_answers_unavailable_within_the_call_budget` holds
the records thread and checks the answer comes inside the call's deadline.
The 16 is not measured; it only keeps a stuck store from collecting work.

## One job's total budget

A job is one call the worker takes from the queue: its export and every
call deferred from it (`views.opened` and `views.message`), which the
host delivers after the export returns. Each of those has its own
deadline, but a plugin may chain them: each `views.opened` can open another
view, up to 1,024 deferred calls (`MAX_DEFERRED_PER_EXPORT`). So a job also
has a total budget, `JOB_BUDGET`, 2 s. The budget bounds the whole job, and
does not grow with the number of deferred calls, because a budget that grew
would let one job hold shutdown without limit again. A plugin whose single
job spends more than 2 s across its calls is treated as stuck
(`Failure::OverBudget`), even if each call kept to its own deadline. At
twice the longest export deadline, no single call reaches the budget alone.

The session checks the budget, and the worker's stop signal, between
deferred calls, not during one, and again before `complete` delivers an
outcome. A job can therefore end up to one deferred call's deadline (at
most 500 ms, `views.opened` or `views.message`) after its budget runs out.
So shutdown waits, for each worker side by side, at most the job
budget plus one deferred call's deadline plus `lifecycle.deactivate`'s
deadline (about 2.75 s), provided its host services honour their
deadlines.

A stop request is not a failure: the deferred calls still queued are
dropped, the instance is kept, and the worker then runs
`lifecycle.deactivate` under its own 250 ms deadline as usual. Each caller
of a dropped call is told: a queued view message is answered
`unavailable`, and `complete`, which delivers an outcome through
`events.deliver` after anything already deferred, answers `unavailable`
instead once a stop is requested. Every way out of a worker (a stop, a
trap, the budget or a flood of deferred calls) closes each view still
open with the services before the store is dropped.

## The queue

`PLUGIN_QUEUE_CAPACITY` is 64 calls per plugin. The workload shows a healthy
plugin answers a full queue in about 2 ms, so a queue only fills when a
plugin is slow or stuck. The size then decides two things:

- **How much work can pile up behind a stuck plugin.** For a *stuck*
  plugin, the queue's size does not change how long a caller waits: when
  the stuck call reaches its deadline (or its job's budget), the plugin is
  discarded and every queued call is answered `unavailable` at once. A
  *slow* plugin is different. If every call finishes just inside its
  deadline, none is stopped, and the last of 64 queued calls waits for the
  63 before it. The queue bounds that wait; it does not make it short. The
  UI never waits either way, because it polls `Pending::try_answer`.
- **The largest legitimate burst it absorbs.** The workload's largest
  burst is a palette form being typed into (one validation per keystroke,
  about 30 for a long field) while a plugin tab sends its start-up
  messages and the search slice pulls a feed: about 32 calls. 64 is twice
  that.

Rejected: an unbounded queue, which lets a stuck plugin collect calls
without limit; and a queue of one, which would refuse a keystroke's
validation while the previous one is still running.

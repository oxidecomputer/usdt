# Tracing `tokio`

Many DTrace examples you'll find make extensive use of thread-local variables, prefixed with
`self->`. For example, you'll often see scripts like this:

```dtrace
// !!! Don't do this with tokio!!!

pid$target::my_func:entry
{
	self->arg = arg0;
}

pid$target::my_func:return
/self->arg/
{
	// Do something with self->arg
	self->arg = 0;
}
```

This uses the system concept of a thread, and--while `async` and `tokio` docs go to great lengths to
liken a "task" to a "thread"--these are fundamentally different entities! In particular, an `async`
Rust executor (such as `tokio`) will often multiplex tasks over a thread pool. This means that the
system's notion of a thread (as in the example above) is meaningless with regard to tracing `async`.

We can think of DTrace's thread-local variables as shorthand for a global array indexed by the
current thread. So `self->foo` is (conceptually) equivalent to `foo[curthread]`. Since a task may be
run on various threads, the current thread isn't a useful key.
**In order to correlate activies across calls, you need some other unique ID**.

## Aside: tokio tasks

It might seem tempting to use some unique task ID as the stand-in for a thread ID. While Tokio
certainly *could* provide some task-unique identifier, it doesn't... and it wouldn't be that useful
if it did! Threads--it turns out--have an extremely important characteristic for understanding their
execution: a given thread is only in one place at one time. You can interrogate a running process
(or even a dead one in the form of a core file!) and ask where each thread is. This may seem
vacuous, but that *very* useful property does not hold for tasks in `async` Rust!

A task is not really representable by a simple, linear stack, but rather requires a tree: a task can
be in multiple places at once! How? The simplest form is via the `join!` macro which explicitly
executes multiple `Future`s in parallel, each of which may make independent progress. So, where is a
task? It may effectively be in several places at once which means that the task doesn't present a
useful way to correlate events.

## Unique IDs

Rather than correlating events by thread, we instead need some other unique ID. Your USDT probes may
already have a unique ID (e.g. a transaction ID), but if it doesn't, `usdt` provides a lightweight
mechanism for adding a unique ID to correlate events. See the [docs for
`usdt::UniqueId`](https://docs.rs/usdt/0.3.5/usdt/struct.UniqueId.html).

If we have a collection of USDT probes whose first argument is a unique ID, we can use it like this
in D:

```dtrace
my_prov$target:::event-start
{
	event[arg0] = 1;
}

my_prov$target:::event-done
/event[arg0]/
{
	// Trace stuff of interest
	event[arg0] = 0;
}
```

Note that this D uses a **global** associative array whose key is the unique ID. We do **not** use
thread-local variables because (as noted exhaustively) the task may run on other threads and other
tasks may run on this thread.



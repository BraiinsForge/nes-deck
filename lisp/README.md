# Retro Deck policy worker

This directory contains trusted Common Lisp behavior that the Rust appliance
runtime will load in a supervised child process. It is deliberately not a
device runtime: display buffers, input descriptors, clocks, audio devices,
processes, and filesystem authority stay in Rust.

Rust supervises one resident Lisp child on an asynchronous worker thread. Each
application loads the tracked policy and local startup files once, then sends
at most one bounded decision request at a time. The timer reuses that loaded
worker across rounds and starts a replacement only after an actual failure.
Product event loops only hand off a request and poll its outcome, so they never
wait for Lisp startup, file loading, evaluation, or pipe I/O.

The appliance worker directly source-loads the eight tracked policy files in
their declared order, installs its default hooks, and then loads root-owned
local files from
`RETRO_DECK_LISP_SITE_DIR` in lexical order. Production uses
`/mnt/data/nes-deck/lisp/site.d`. The worker emits its `:ready` message only
after every startup file loads successfully. The Rust host must terminate or
replace a worker that fails startup, exceeds its deadline, crashes, or returns
invalid data.

The direct loader deliberately avoids initializing ASDF on the memory-limited
Deck. `retro-deck.asd` remains the development and test system definition. The
Rust supervisor gives source-loaded ECL a bounded 15-second startup window;
policy requests retain their separate 250-millisecond deadline after readiness.

Local files use the same package and replace behavior by registering a hook:

```lisp
(in-package #:retro-deck)

(register-policy-hook
 :ten-seconds/result
 (lambda (arguments)
   (declare (ignore arguments))
   '(:display-centiseconds 1000 :cue :exact)))
```

The site directory is persistent local state. It is excluded from Git,
preserved by deployment, and never exposed through the ROM uploader.

The native dashboard queues `:dashboard/startup` as soon as the worker starts;
the supervisor delivers it after Lisp has loaded the tracked and site files,
then terminates the worker. The hook returns ordered
`(kind title color)` rows and a bounded raw gamepad profile. A local file can
patch product behavior without gaining executable, device, or path authority:

```lisp
(in-package #:retro-deck)

(register-policy-hook
 :dashboard/startup
 (lambda (arguments)
   (unless (null arguments)
     (error "unexpected dashboard arguments"))
   '(:applications
     ((:lisp "LISP" "#AFD75F")
      (:terminal "SHELL" "#5F87AF")
      (:reboot "REBOOT" "#D75F5F"))
     :gamepad
     ((:button 290 :confirm)
      (:button 294 :back)
      (:axis 0 :left :right)
      (:axis 1 :up :down)))))
```

Accepted kinds are `:lua`, `:lisp`, `:python`, `:scheme`, `:chiptunes`,
`:terminal`, and `:reboot`, at most once each. Rust validates the title and
xterm-256 color and supplies each stable identity itself. If the worker or a
local override fails, the dashboard keeps its base ROM catalog instead of
failing startup. Gamepad rows are `(:button CODE :ACTION)` or
`(:axis CODE :NEGATIVE-ACTION :POSITIVE-ACTION)`; accepted actions are
`:up`, `:down`, `:left`, `:right`, `:confirm`, and `:back`. Rust validates the
profile once, then handles report timing locally without calling Lisp from the
input path.

## Wire protocol

Standard input and output carry one bounded S-expression per UTF-8 line. The
data vocabulary is limited to proper lists, signed 64-bit integers, strings
without control characters, canonical keywords, `t`, and `nil`. Keyword names
use ASCII letters, digits, and `-/+*<>=!?_.`, matching the Rust codec. Reader
evaluation, reader macros, arbitrary symbols, dotted lists, trailing forms,
excessive depth, excessive values, and messages over 64 KiB are rejected.

A request and successful response look like this:

```lisp
(:request :version 1 :id 42 :hook :ten-seconds/result
 :arguments (:elapsed-centiseconds 1000 :input :controller-a))

(:response :version 1 :id 42 :status :ok
 :value (:display-centiseconds 1000 :cue :exact))
```

The protocol is private IPC between the Rust parent and its child. It is not a
network API and does not grant local Lisp code access to the parent's handles.

## Tests

Run the host tests with:

```sh
sbcl --script lisp/tests/run.lisp
```

Run the worker manually with SBCL using:

```sh
sbcl --script lisp/run-worker.lisp
```

The Deck launches the same entry point with ECL and disables user startup
files:

```sh
ecl --norc --shell lisp/run-worker.lisp
```

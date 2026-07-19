# Retro Deck policy worker

This directory contains trusted Common Lisp behavior that the Rust appliance
runtime will load in a supervised child process. It is deliberately not a
device runtime: display buffers, input descriptors, clocks, audio devices,
processes, and filesystem authority stay in Rust.

The worker loads the tracked `retro-deck` ASDF system, installs its default
hooks, and then loads root-owned local files from
`RETRO_DECK_LISP_SITE_DIR` in lexical order. Production uses
`/mnt/data/nes-deck/lisp/site.d`. The worker emits its `:ready` message only
after every startup file loads successfully. The Rust host must terminate or
replace a worker that fails startup, exceeds its deadline, crashes, or returns
invalid data.

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

## Wire protocol

Standard input and output carry one bounded S-expression per UTF-8 line. The
data vocabulary is limited to proper lists, signed 64-bit integers, strings
without control characters, keywords, `t`, and `nil`. Reader evaluation,
reader macros, arbitrary symbols, dotted lists, trailing forms, excessive
depth, excessive values, and messages over 64 KiB are rejected.

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

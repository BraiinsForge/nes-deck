(in-package #:retro-deck)


;;;; -- Dashboard startup policy --

(defvar *dashboard-default-applications*
  '((:lua "LUA REPL" "#5F87FF")
    (:lisp "LISP REPL" "#AFD75F")
    (:python "PYTHON REPL" "#FFD700")
    (:scheme "SCHEME REPL" "#87D787")
    (:chiptunes "CHIPTUNES" "#FF8700")
    (:terminal "TERMINAL" "#5F87AF")
    (:reboot "REBOOT" "#D75F5F"))
  "Fallback ordered Deck applications returned at dashboard startup.")


(defun dashboard--default-applications (arguments)
  "Return the tracked application rows when ARGUMENTS is empty."
  (unless (null arguments)
    (error 'policy-hook-error
           :hook-name :dashboard/applications
           :reason "dashboard startup accepts no arguments"))
  (copy-tree *dashboard-default-applications*))

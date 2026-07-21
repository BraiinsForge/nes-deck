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

(defvar *dashboard-default-gamepad*
  '((:button 288 :back)       ; Y
    (:button 289 :back)       ; B
    (:button 290 :confirm)    ; A
    (:button 291 :confirm)    ; X
    (:button 294 :back)       ; Select
    (:button 295 :confirm)    ; Start
    (:axis 0 :left :right)    ; horizontal D-pad
    (:axis 1 :up :down))      ; vertical D-pad
  "Raw THEGamepad bindings interpreted by the native dashboard.")


(defun dashboard--default-startup (arguments)
  "Return tracked application and gamepad policy when ARGUMENTS is empty."
  (unless (null arguments)
    (error 'policy-hook-error
           :hook-name :dashboard/startup
           :reason "dashboard startup accepts no arguments"))
  (list :applications (copy-tree *dashboard-default-applications*)
        :gamepad (copy-tree *dashboard-default-gamepad*)))

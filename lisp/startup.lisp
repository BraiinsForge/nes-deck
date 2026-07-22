(defpackage #:retrodeck.native
  (:use)
  (:export #:abi-version))

(defpackage #:retrodeck
  (:use #:cl)
  (:import-from #:retrodeck.native #:abi-version)
  (:export #:*menu-sound-cues*
           #:*menu-sound-input-tail-ms*
           #:main
           #:menu-sound-duration-ms
           #:menu-sound-notes))

(in-package #:retrodeck)

(defconstant +native-abi-version+ 1)

(defparameter *menu-sound-cues*
  '((:volume (660 60) (880 60))
    (:previous (523 35))
    (:next (659 35))
    (:confirm (659 25) (880 30))
    (:back (659 25) (440 30))))

(defparameter *menu-sound-input-tail-ms* 60)

(defun menu-sound-notes (cue)
  (copy-tree (cdr (or (assoc cue *menu-sound-cues*)
                      (assoc :back *menu-sound-cues*)))))

(defun menu-sound-duration-ms (cue)
  (reduce #'+ (menu-sound-notes cue) :key #'second))

(defun main ()
  (unless (= (abi-version) +native-abi-version+)
    (error "Native ABI mismatch"))
  (format t "retrodeck: Common Lisp startup loaded~%")
  (finish-output)
  0)

(let ((local (merge-pathnames "local.lisp" *load-truename*)))
  (when (probe-file local)
    (load local :verbose nil :print nil)))

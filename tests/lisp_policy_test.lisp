(defpackage #:retrodeck.test
  (:use #:cl))

(in-package #:retrodeck.test)

(defparameter *play-status* 1)
(defparameter *play-arguments* nil)
(defparameter *active-status* 0)
(defparameter *stop-count* 0)
(defparameter *finish-count* 0)

(defpackage #:retrodeck.native
  (:use)
  (:export #:abi-version
           #:audio-active-p
           #:finish-audio
           #:play-tones
           #:stop-audio))

(setf (symbol-function (find-symbol "ABI-VERSION" "RETRODECK.NATIVE"))
      (lambda () 2)
      (symbol-function (find-symbol "AUDIO-ACTIVE-P" "RETRODECK.NATIVE"))
      (lambda () *active-status*)
      (symbol-function (find-symbol "PLAY-TONES" "RETRODECK.NATIVE"))
      (lambda (&rest arguments)
        (setf *play-arguments* arguments)
        *play-status*)
      (symbol-function (find-symbol "STOP-AUDIO" "RETRODECK.NATIVE"))
      (lambda () (incf *stop-count*) 0)
      (symbol-function (find-symbol "FINISH-AUDIO" "RETRODECK.NATIVE"))
      (lambda () (incf *finish-count*) 0))

(load (truename (merge-pathnames "../lisp/startup.lisp" *load-truename*))
      :verbose nil :print nil)

(assert (equal (retrodeck:menu-sound-notes :volume)
               '((660 60) (880 60))))
(assert (equal (retrodeck:menu-sound-notes :previous) '((523 35))))
(assert (equal (retrodeck:menu-sound-notes :next) '((659 35))))
(assert (equal (retrodeck:menu-sound-notes :confirm)
               '((659 25) (880 30))))
(assert (equal (retrodeck:menu-sound-notes :unknown)
               '((659 25) (440 30))))
(assert (= (retrodeck:menu-sound-duration-ms :volume) 120))
(assert (= (retrodeck:menu-sound-duration-ms :confirm) 55))
(assert (= retrodeck:*menu-sound-input-tail-ms* 60))

(let ((before (retrodeck::monotonic-ms)))
  (setf *play-status* 1)
  (assert (retrodeck:play-menu-sound :confirm 42))
  (let ((after (retrodeck::monotonic-ms)))
    (assert (<= (+ before 115)
                retrodeck::*menu-sound-input-until-ms*
                (+ after 115)))))
(assert (equal *play-arguments* '(659 25 880 30 42)))

(setf *play-status* 1)
(assert (retrodeck:play-menu-sound :previous 17))
(assert (equal *play-arguments* '(523 35 0 0 17)))

(setf retrodeck::*menu-sound-input-until-ms* 77
      *play-status* 2)
(assert (retrodeck:play-menu-sound :next 42))
(assert (= retrodeck::*menu-sound-input-until-ms* 77))

(setf *play-status* 0)
(assert (not (retrodeck:play-menu-sound :next 42)))
(assert (= retrodeck::*menu-sound-input-until-ms* 77))

(setf *play-arguments* nil)
(assert (retrodeck:play-menu-sound :next 0))
(assert (null *play-arguments*))

(setf *active-status* 1
      retrodeck::*menu-sound-input-until-ms* 0)
(assert (retrodeck:menu-sound-blocks-input-p :controller 100))
(assert (not (retrodeck:menu-sound-blocks-input-p :touch 100)))
(assert (not (retrodeck:menu-sound-blocks-input-p :keyboard 100)))

(setf *active-status* 0
      retrodeck::*menu-sound-input-until-ms* 100)
(assert (retrodeck:menu-sound-blocks-input-p :controller 99))
(assert (not (retrodeck:menu-sound-blocks-input-p :controller 100)))

(setf retrodeck::*menu-sound-input-until-ms* 100)
(assert (retrodeck:stop-menu-sound))
(assert (= *stop-count* 1))
(assert (= retrodeck::*menu-sound-input-until-ms* 0))

(setf retrodeck::*menu-sound-input-until-ms* 100)
(assert (retrodeck:finish-menu-sound))
(assert (= *finish-count* 1))
(assert (= retrodeck::*menu-sound-input-until-ms* 0))

(format t "Lisp policy tests passed.~%")

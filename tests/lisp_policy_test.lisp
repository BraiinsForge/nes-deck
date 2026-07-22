(defpackage #:retrodeck.test
  (:use #:cl))

(in-package #:retrodeck.test)

(defparameter *play-status* 1)
(defparameter *play-arguments* nil)
(defparameter *active-status* 0)
(defparameter *stop-count* 0)
(defparameter *finish-count* 0)
(defparameter *wayland-open-status* 1)
(defparameter *wayland-close-count* 0)
(defparameter *wayland-present-status* 1)
(defparameter *wayland-present-color* nil)
(defparameter *wayland-dispatch-result* 0)
(defparameter *wayland-dispatch-timeout* nil)
(defparameter *wayland-touch* nil)
(defparameter *wayland-size* nil)
(defparameter *wayland-shutdown-status* 0)

(defpackage #:retrodeck.native
  (:use)
  (:export #:abi-version
           #:audio-active-p
           #:finish-audio
           #:play-tones
           #:stop-audio
           #:wayland-close
           #:wayland-dispatch
           #:wayland-next-touch
           #:wayland-open-widget
           #:wayland-present-solid
           #:wayland-shutdown-p
           #:wayland-size))

(setf (symbol-function (find-symbol "ABI-VERSION" "RETRODECK.NATIVE"))
      (lambda () 3)
      (symbol-function (find-symbol "AUDIO-ACTIVE-P" "RETRODECK.NATIVE"))
      (lambda () *active-status*)
      (symbol-function (find-symbol "PLAY-TONES" "RETRODECK.NATIVE"))
      (lambda (&rest arguments)
        (setf *play-arguments* arguments)
        *play-status*)
      (symbol-function (find-symbol "STOP-AUDIO" "RETRODECK.NATIVE"))
      (lambda () (incf *stop-count*) 0)
      (symbol-function (find-symbol "FINISH-AUDIO" "RETRODECK.NATIVE"))
      (lambda () (incf *finish-count*) 0)
      (symbol-function (find-symbol "WAYLAND-OPEN-WIDGET" "RETRODECK.NATIVE"))
      (lambda () *wayland-open-status*)
      (symbol-function (find-symbol "WAYLAND-CLOSE" "RETRODECK.NATIVE"))
      (lambda () (incf *wayland-close-count*) 0)
      (symbol-function (find-symbol "WAYLAND-PRESENT-SOLID" "RETRODECK.NATIVE"))
      (lambda (color)
        (setf *wayland-present-color* color)
        *wayland-present-status*)
      (symbol-function (find-symbol "WAYLAND-DISPATCH" "RETRODECK.NATIVE"))
      (lambda (timeout-ms)
        (setf *wayland-dispatch-timeout* timeout-ms)
        *wayland-dispatch-result*)
      (symbol-function (find-symbol "WAYLAND-NEXT-TOUCH" "RETRODECK.NATIVE"))
      (lambda () *wayland-touch*)
      (symbol-function (find-symbol "WAYLAND-SIZE" "RETRODECK.NATIVE"))
      (lambda () *wayland-size*)
      (symbol-function (find-symbol "WAYLAND-SHUTDOWN-P" "RETRODECK.NATIVE"))
      (lambda () *wayland-shutdown-status*))

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

(assert (retrodeck:open-wayland-widget))
(assert (retrodeck:close-wayland))
(assert (= *wayland-close-count* 1))
(assert (retrodeck:present-wayland-solid #x123456))
(assert (= *wayland-present-color* #x123456))

(setf *wayland-dispatch-result* 4)
(assert (= (retrodeck:dispatch-wayland 25) 4))
(assert (= *wayland-dispatch-timeout* 25))
(setf *wayland-dispatch-result* -1)
(assert (null (retrodeck:dispatch-wayland)))

(setf *wayland-touch* '(1279 0 1 0 0))
(assert (equal (retrodeck:next-wayland-touch)
               '(1279 0 t nil nil)))
(setf *wayland-touch* nil
      *wayland-size* '(1280 480))
(assert (null (retrodeck:next-wayland-touch)))
(assert (equal (retrodeck:current-wayland-size) '(1280 480)))
(assert (not (retrodeck:wayland-shutdown-requested-p)))
(setf *wayland-shutdown-status* 1)
(assert (retrodeck:wayland-shutdown-requested-p))

(format t "Lisp policy tests passed.~%")

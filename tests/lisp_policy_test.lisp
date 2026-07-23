(defpackage #:retrodeck.test
  (:use #:cl))

(in-package #:retrodeck.test)

(defparameter *play-status* 1)
(defparameter *play-arguments* nil)
(defparameter *record-interaction* nil)
(defparameter *interaction-trace* nil)
(defparameter *active-status* 0)
(defparameter *stop-count* 0)
(defparameter *finish-count* 0)
(defparameter *canvas-clear-status* 1)
(defparameter *canvas-clear-color* nil)
(defparameter *canvas-hash-words* '(1 2 3 4))
(defparameter *canvas-glyph-status* 1)
(defparameter *canvas-glyph-arguments* nil)
(defparameter *canvas-glyph-calls* nil)
(defparameter *canvas-fill-status* 1)
(defparameter *canvas-fill-arguments* nil)
(defparameter *projection-status* 1)
(defparameter *projection-arguments* nil)
(defparameter *projected-text-status* 1)
(defparameter *projected-text-arguments* nil)
(defparameter *projected-text-calls* nil)
(defparameter *text-mask-result* 0)
(defparameter *text-mask-arguments* nil)
(defparameter *text-mask-calls* nil)
(defparameter *text-mask-clear-count* 0)
(defparameter *regular-file-result* nil)
(defparameter *regular-file-arguments* nil)
(defparameter *canvas-fill-calls* nil)
(defparameter *canvas-raster-status* 1)
(defparameter *canvas-raster-arguments* nil)
(defparameter *canvas-raster-calls* nil)
(defparameter *raster-clear-count* 0)
(defparameter *raster-cover-result* 0)
(defparameter *raster-cover-arguments* nil)
(defparameter *raster-cover-calls* nil)
(defparameter *raster-png-result* 0)
(defparameter *raster-png-arguments* nil)
(defparameter *raster-png-calls* nil)
(defparameter *evdev-controls-scan-result* '(0 0))
(defparameter *evdev-controls-close-count* 0)
(defparameter *evdev-controls-dispatch-result* '(0 0))
(defparameter *evdev-controls-dispatch-timeout* nil)
(defparameter *evdev-controls* nil)
(defparameter *evdev-open-status* 1)
(defparameter *evdev-close-count* 0)
(defparameter *evdev-dispatch-result* 0)
(defparameter *evdev-dispatch-timeout* nil)
(defparameter *evdev-touch* nil)
(defparameter *fbdev-open-status* 1)
(defparameter *fbdev-close-count* 0)
(defparameter *fbdev-canvas-status* 1)
(defparameter *fbdev-present-status* 1)
(defparameter *fbdev-present-color* nil)
(defparameter *fbdev-size* nil)
(defparameter *wayland-open-status* 1)
(defparameter *wayland-close-count* 0)
(defparameter *wayland-canvas-status* 1)
(defparameter *wayland-present-status* 1)
(defparameter *wayland-present-color* nil)
(defparameter *wayland-dispatch-result* 0)
(defparameter *wayland-dispatch-timeout* nil)
(defparameter *wayland-touch* nil)
(defparameter *wayland-size* nil)
(defparameter *wayland-shutdown-status* 0)
(defparameter *terminal-result* '(1 0 0 -1 nil))
(defparameter *terminal-arguments* nil)

(defpackage #:retrodeck.native
  (:use)
  (:export #:abi-version
           #:audio-active-p
           #:canvas-clear
           #:canvas-configure-projection
           #:canvas-draw-glyph
           #:canvas-draw-projected-text
           #:canvas-draw-raster
           #:canvas-fill-rect
           #:canvas-rgb565-hash-words
           #:evdev-controls-close
           #:evdev-controls-dispatch
           #:evdev-controls-scan
           #:evdev-next-control
           #:evdev-next-touch
           #:evdev-touch-close
           #:evdev-touch-dispatch
           #:evdev-touch-open
           #:fbdev-close
           #:fbdev-open
           #:fbdev-present-canvas
           #:fbdev-present-solid
           #:fbdev-size
           #:finish-audio
           #:play-tones
           #:raster-clear
           #:raster-load-cover
           #:raster-load-png
           #:read-regular-file
           #:run-terminal
           #:stop-audio
           #:text-mask-clear
           #:text-mask-load
           #:wayland-close
           #:wayland-dispatch
           #:wayland-next-touch
           #:wayland-open-widget
           #:wayland-present-canvas
           #:wayland-present-solid
           #:wayland-shutdown-p
           #:wayland-size))

(setf (symbol-function (find-symbol "ABI-VERSION" "RETRODECK.NATIVE"))
      (lambda () 12)
      (symbol-function (find-symbol "AUDIO-ACTIVE-P" "RETRODECK.NATIVE"))
      (lambda () *active-status*)
      (symbol-function (find-symbol "PLAY-TONES" "RETRODECK.NATIVE"))
      (lambda (&rest arguments)
        (setf *play-arguments* arguments)
        (when *record-interaction*
          (push :sound *interaction-trace*))
        *play-status*)
      (symbol-function (find-symbol "STOP-AUDIO" "RETRODECK.NATIVE"))
      (lambda () (incf *stop-count*) 0)
      (symbol-function (find-symbol "FINISH-AUDIO" "RETRODECK.NATIVE"))
      (lambda () (incf *finish-count*) 0)
      (symbol-function (find-symbol "CANVAS-CLEAR" "RETRODECK.NATIVE"))
      (lambda (color)
        (setf *canvas-clear-color* color)
        (when *record-interaction*
          (push :render *interaction-trace*))
        *canvas-clear-status*)
      (symbol-function (find-symbol "CANVAS-RGB565-HASH-WORDS"
                                    "RETRODECK.NATIVE"))
       (lambda () *canvas-hash-words*)
       (symbol-function (find-symbol "CANVAS-CONFIGURE-PROJECTION"
                                    "RETRODECK.NATIVE"))
       (lambda (&rest arguments)
         (setf *projection-arguments* arguments)
         *projection-status*)
       (symbol-function (find-symbol "CANVAS-DRAW-PROJECTED-TEXT"
                                    "RETRODECK.NATIVE"))
       (lambda (&rest arguments)
         (setf *projected-text-arguments* arguments)
         (push arguments *projected-text-calls*)
         *projected-text-status*)
       (symbol-function (find-symbol "CANVAS-DRAW-GLYPH" "RETRODECK.NATIVE"))
      (lambda (&rest arguments)
        (setf *canvas-glyph-arguments* arguments)
        (push arguments *canvas-glyph-calls*)
        *canvas-glyph-status*)
      (symbol-function (find-symbol "CANVAS-FILL-RECT" "RETRODECK.NATIVE"))
      (lambda (&rest arguments)
        (setf *canvas-fill-arguments* arguments)
        (push arguments *canvas-fill-calls*)
        *canvas-fill-status*)
      (symbol-function (find-symbol "CANVAS-DRAW-RASTER" "RETRODECK.NATIVE"))
       (lambda (&rest arguments)
         (setf *canvas-raster-arguments* arguments)
         (push arguments *canvas-raster-calls*)
         *canvas-raster-status*)
       (symbol-function (find-symbol "RASTER-CLEAR" "RETRODECK.NATIVE"))
       (lambda () (incf *raster-clear-count*) 1)
       (symbol-function (find-symbol "RASTER-LOAD-COVER" "RETRODECK.NATIVE"))
       (lambda (&rest arguments)
         (setf *raster-cover-arguments* arguments)
         (push arguments *raster-cover-calls*)
         *raster-cover-result*)
       (symbol-function (find-symbol "RASTER-LOAD-PNG" "RETRODECK.NATIVE"))
       (lambda (&rest arguments)
         (setf *raster-png-arguments* arguments)
         (push arguments *raster-png-calls*)
         *raster-png-result*)
       (symbol-function (find-symbol "READ-REGULAR-FILE" "RETRODECK.NATIVE"))
        (lambda (&rest arguments)
          (setf *regular-file-arguments* arguments)
          *regular-file-result*)
        (symbol-function (find-symbol "RUN-TERMINAL" "RETRODECK.NATIVE"))
        (lambda (&rest arguments)
          (setf *terminal-arguments* arguments)
          *terminal-result*)
        (symbol-function (find-symbol "TEXT-MASK-CLEAR" "RETRODECK.NATIVE"))
        (lambda () (incf *text-mask-clear-count*) 1)
        (symbol-function (find-symbol "TEXT-MASK-LOAD" "RETRODECK.NATIVE"))
        (lambda (&rest arguments)
          (setf *text-mask-arguments* arguments)
          (push arguments *text-mask-calls*)
          *text-mask-result*)
        (symbol-function (find-symbol "EVDEV-CONTROLS-SCAN" "RETRODECK.NATIVE"))
         (lambda () *evdev-controls-scan-result*)
         (symbol-function (find-symbol "EVDEV-CONTROLS-CLOSE" "RETRODECK.NATIVE"))
         (lambda () (incf *evdev-controls-close-count*) 0)
         (symbol-function (find-symbol "EVDEV-CONTROLS-DISPATCH"
                                       "RETRODECK.NATIVE"))
         (lambda (timeout-ms)
           (setf *evdev-controls-dispatch-timeout* timeout-ms)
           *evdev-controls-dispatch-result*)
         (symbol-function (find-symbol "EVDEV-NEXT-CONTROL" "RETRODECK.NATIVE"))
         (lambda () (pop *evdev-controls*))
         (symbol-function (find-symbol "EVDEV-TOUCH-OPEN" "RETRODECK.NATIVE"))
         (lambda () *evdev-open-status*)
         (symbol-function (find-symbol "EVDEV-TOUCH-CLOSE" "RETRODECK.NATIVE"))
         (lambda () (incf *evdev-close-count*) 0)
         (symbol-function (find-symbol "EVDEV-TOUCH-DISPATCH" "RETRODECK.NATIVE"))
         (lambda (timeout-ms)
           (setf *evdev-dispatch-timeout* timeout-ms)
           *evdev-dispatch-result*)
         (symbol-function (find-symbol "EVDEV-NEXT-TOUCH" "RETRODECK.NATIVE"))
         (lambda () *evdev-touch*)
         (symbol-function (find-symbol "FBDEV-OPEN" "RETRODECK.NATIVE"))
      (lambda () *fbdev-open-status*)
      (symbol-function (find-symbol "FBDEV-CLOSE" "RETRODECK.NATIVE"))
      (lambda () (incf *fbdev-close-count*) 0)
      (symbol-function (find-symbol "FBDEV-PRESENT-CANVAS" "RETRODECK.NATIVE"))
      (lambda () *fbdev-canvas-status*)
      (symbol-function (find-symbol "FBDEV-PRESENT-SOLID" "RETRODECK.NATIVE"))
      (lambda (color)
        (setf *fbdev-present-color* color)
        *fbdev-present-status*)
      (symbol-function (find-symbol "FBDEV-SIZE" "RETRODECK.NATIVE"))
      (lambda () *fbdev-size*)
      (symbol-function (find-symbol "WAYLAND-OPEN-WIDGET" "RETRODECK.NATIVE"))
      (lambda () *wayland-open-status*)
      (symbol-function (find-symbol "WAYLAND-CLOSE" "RETRODECK.NATIVE"))
      (lambda () (incf *wayland-close-count*) 0)
      (symbol-function (find-symbol "WAYLAND-PRESENT-CANVAS" "RETRODECK.NATIVE"))
      (lambda () *wayland-canvas-status*)
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

(defun signals-type-error-p (function)
  (handler-case
      (progn (funcall function) nil)
    (type-error () t)))

(defun signals-error-p (function)
  (handler-case
      (progn (funcall function) nil)
    (error () t)))

(defun test-file-string (path)
  (with-open-file (input path)
    (let ((contents (make-string (file-length input))))
      (read-sequence contents input)
      contents)))

(defun decode-native-unsigned-64 (text)
  (assert (= (length text) 16))
  (parse-integer text :radix 16))

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

(assert (retrodeck:clear-canvas #x121212))
(assert (= *canvas-clear-color* #x121212))
(assert (= (retrodeck:canvas-rgb565-hash) #x0001000200030004))
(assert (retrodeck:draw-canvas-glyph -4 8 65 2 #xfe6c27))
(assert (equal *canvas-glyph-arguments* '(-4 8 65 2 #xfe6c27)))
(assert (retrodeck:draw-canvas-glyph 0 0 0 1 0))
(assert (retrodeck:draw-canvas-glyph 0 0 255 1 #xffffff))
(setf *canvas-glyph-status* 0)
(assert (not (retrodeck:draw-canvas-glyph 0 0 65 1 0)))
(setf *canvas-glyph-status* 1
      *canvas-glyph-arguments* nil)
(dolist (arguments '((#x80000000 0 65 1 0)
                     (0 0 256 1 0)
                     (0 0 65 0 0)
                     (0 0 65 #x100000000 0)))
  (assert (signals-type-error-p
           (lambda () (apply #'retrodeck:draw-canvas-glyph arguments)))))
(assert (null *canvas-glyph-arguments*))
(assert (retrodeck:fill-canvas-rect -4 8 12 16 #xfe6c27))
(assert (equal *canvas-fill-arguments* '(-4 8 12 16 #xfe6c27)))
(setf *canvas-fill-arguments* nil)
(assert (signals-type-error-p
         (lambda () (retrodeck:fill-canvas-rect #x80000000 0 1 1 0))))
(assert (signals-type-error-p
         (lambda () (retrodeck:fill-canvas-rect 0 0 #x100000000 1 0))))
(assert (null *canvas-fill-arguments*))

(setf *regular-file-result* "project\trole\tlicense\n")
(assert (string= (retrodeck:read-bounded-regular-file
                  "/tmp/credits.tsv" 1 32768)
                 *regular-file-result*))
(assert (equal *regular-file-arguments* '("/tmp/credits.tsv" 1 32768)))
(assert (signals-type-error-p
         (lambda () (retrodeck:read-bounded-regular-file "/tmp/x" -1 2))))
(assert (signals-type-error-p
         (lambda ()
           (retrodeck:read-bounded-regular-file "/tmp/x" 1 4194305))))

(setf *text-mask-result* 17)
(assert (= (retrodeck:load-text-mask "HH" 4) 17))
(assert (equal *text-mask-arguments* '("HH" 4)))
(assert (retrodeck:configure-text-projection
         2000 1 20 8044 420 4000 56 72 104 210 480 #xffffaf))
(assert (= (decode-native-unsigned-64 (first *projection-arguments*))
           2000))
(assert (equal (rest *projection-arguments*)
               '(1 20 8044 420 4000 56 72 104 210 480 #xffffaf)))
(assert (retrodeck:configure-text-projection
         600000000 1 20 8044 420 4000 56 72 104 210 480 #xffffaf))
(assert (= (decode-native-unsigned-64 (first *projection-arguments*))
           600000000))
(assert (retrodeck:draw-projected-text 17 44))
(assert (equal *projected-text-arguments* '(17 44)))
(assert (retrodeck:clear-text-mask-cache))
(assert (= *text-mask-clear-count* 1))
(setf *projection-status* 0
      *projected-text-status* 0)
(assert (not (retrodeck:configure-text-projection
              0 1 20 4044 420 4000 56 72 104 210 480 0)))
(assert (not (retrodeck:draw-projected-text 17 0)))
(setf *projection-status* 1
      *projected-text-status* 1)
(dolist (function
         (list (lambda () (retrodeck:load-text-mask "" 0))
               (lambda ()
                 (retrodeck:configure-text-projection
                  -1 1 20 4044 420 4000 56 72 104 210 480 0))
               (lambda () (retrodeck:draw-projected-text 0 0))))
  (assert (signals-type-error-p function)))

(setf *raster-cover-result* 17)
(assert (= (retrodeck:load-cover-raster #P"/tmp/cover.png" #x5f87ff) 17))
(assert (equal *raster-cover-arguments* '("/tmp/cover.png" #x5f87ff)))
(setf *raster-png-result* 18)
(assert (= (retrodeck:load-png-raster "/tmp/icon.png" 23 23) 18))
(assert (equal *raster-png-arguments* '("/tmp/icon.png" 23 23)))
(assert (retrodeck:draw-canvas-raster 18 -4 8 50 50))
(assert (equal *canvas-raster-arguments* '(18 -4 8 50 50)))
(setf *canvas-raster-status* 0)
(assert (not (retrodeck:draw-canvas-raster 18 0 0 1 1)))
(setf *canvas-raster-status* 1
      *raster-cover-result* 0
      *raster-png-result* 0)
(dolist (function (list (lambda () (retrodeck:load-cover-raster "/tmp/x" #x1000000))
                        (lambda () (retrodeck:load-png-raster "/tmp/x" 0 1))
                        (lambda () (retrodeck:load-png-raster "/tmp/x" 2049 1))
                        (lambda () (retrodeck:draw-canvas-raster 0 0 0 1 1))))
  (assert (signals-type-error-p function)))

(assert (string= (retrodeck:display-ascii "AČz") "A?z"))
(assert (= (retrodeck:bitmap-text-width "" 2) 0))
(assert (= (retrodeck:bitmap-text-width "AB" 2) 22))
(assert (= (retrodeck:fit-text-scale "ABCDE" 29 3 1) 1))
(assert (= (retrodeck:fit-text-scale "ABCDE" 1 3 2) 2))
(assert (string= (retrodeck:fit-text-width "ABCDEFGHIJ" 29 1) "AB..."))
(assert (string= (retrodeck:fit-text-width "AB" 1 1) ""))

(setf *canvas-glyph-calls* nil)
(assert (retrodeck:draw-text 100 100 "AČ" 2 #xeeeeee))
(assert (equal (nreverse *canvas-glyph-calls*)
               '((100 100 65 2 #xeeeeee)
                 (112 100 63 2 #xeeeeee))))
(setf *canvas-glyph-calls* nil)
(assert (retrodeck:draw-centered-text 10 20 30 40 "AB" 2 #xeeeeee))
(assert (equal (nreverse *canvas-glyph-calls*)
               '((14 33 65 2 #xeeeeee)
                 (26 33 66 2 #xeeeeee))))

(setf *canvas-fill-calls* nil)
(assert (retrodeck:stroke-canvas-rect 10 20 30 40 2 #xeeeeee))
(assert (equal (nreverse *canvas-fill-calls*)
               '((10 20 30 2 #xeeeeee)
                 (10 58 30 2 #xeeeeee)
                 (10 20 2 40 #xeeeeee)
                 (38 20 2 40 #xeeeeee))))
(setf *canvas-fill-calls* nil)
(assert (retrodeck:fill-pixel-cut-rect 100 100 20 12 4 #xfe6c27))
(assert (equal (nreverse *canvas-fill-calls*)
               '((104 100 12 12 #xfe6c27)
                 (100 104 20 4 #xfe6c27))))
(setf *canvas-fill-calls* nil)
(assert (retrodeck:fill-pixel-cut-rect 100 100 8 12 4 #xfe6c27))
(assert (null *canvas-fill-calls*))
(assert (retrodeck:draw-pixel-panel 100 100 20 20 #x121212 #xfe6c27 4))
(assert (equal (nreverse *canvas-fill-calls*)
               '((104 100 12 20 #xfe6c27)
                 (100 104 20 12 #xfe6c27)
                 (108 104 4 12 #x121212)
                 (104 108 12 4 #x121212))))

(let* ((credits-path
         (truename (merge-pathnames "../deploy/menu/credits.tsv"
                                    *load-truename*)))
       (*regular-file-result* (test-file-string credits-path))
       (credits (retrodeck:load-project-credits credits-path))
       (crawl (retrodeck:make-project-credits-crawl credits)))
  (assert (= (length credits) 32))
  (assert (string= (getf (first credits) :project) "FCEUmm"))
  (assert (string= (getf (car (last credits)) :project)
                   "OpenGameArt contributors"))
  (assert (= (length (getf crawl :lines)) 101))
  (assert (= (length (getf crawl :static-lines)) 32))
  (assert (= (getf crawl :content-height) 5396))
  (assert (equal (first (getf crawl :lines))
                 '(:text "RETRO DECK" :source-y 0
                   :source-width 236 :source-height 28)))
  (assert (equal (car (last (getf crawl :lines)))
                 '(:text "THANK YOU" :source-y 5352
                   :source-width 212 :source-height 28)))
  (assert (every (lambda (line)
                   (and (<= (getf line :source-width) 1040)
                        (= (getf line :source-height) 28)))
                 (getf crawl :lines)))

  (assert (retrodeck:clear-credits-text-mask-cache))
  (setf *text-mask-result* 17
        *text-mask-calls* nil
        *projected-text-calls* nil
        *projection-arguments* nil
        *canvas-fill-calls* nil
        *canvas-glyph-calls* nil)
  (assert (equal (retrodeck:render-project-credits crawl nil 2000)
                 '(:close (1212 12 56 56))))
  (assert (= *canvas-clear-color* #x000000))
  (assert (= (decode-native-unsigned-64 (first *projection-arguments*))
             2000))
  (assert (equal (rest *projection-arguments*)
                 '(1 20 9396 420 4000 56 72 104 210 480 #xffffac)))
  (let ((projected (nreverse *projected-text-calls*)))
    (assert (= (length projected) 101))
    (assert (equal (first projected) '(17 0)))
    (assert (equal (car (last projected)) '(17 5352))))
  (assert (= (length *canvas-fill-calls*) 96))
  (assert (null *canvas-glyph-calls*))
  (assert (<= (length *text-mask-calls*) 101))
  (assert (> (length *text-mask-calls*) 70))

  (setf *canvas-fill-calls* nil
        *canvas-glyph-calls* nil
        *projected-text-calls* nil
        *projection-arguments* nil)
  (retrodeck:render-project-credits crawl t 0)
  (let ((first-fills (reverse *canvas-fill-calls*))
        (first-glyphs (reverse *canvas-glyph-calls*)))
    (assert (= (length first-fills) 14))
    (assert (null *projected-text-calls*))
    (assert (null *projection-arguments*))
    (assert (member '(20 20 70 2 #xffffac) first-glyphs :test #'equal))
    (assert (member '(20 458 47 1 #x949594) first-glyphs :test #'equal))
    (setf *canvas-fill-calls* nil
          *canvas-glyph-calls* nil)
    (retrodeck:render-project-credits crawl t 60000)
    (assert (equal (reverse *canvas-fill-calls*) first-fills))
    (assert (equal (reverse *canvas-glyph-calls*) first-glyphs)))

  (let ((layout '(:close (1212 12 56 56))))
    (assert (eq (retrodeck:credits-target-at layout 1212 12) :close))
    (assert (eq (retrodeck:credits-target-at layout 1267 67) :close))
    (assert (null (retrodeck:credits-target-at layout 1268 67)))
    (multiple-value-bind (pressed effect)
        (retrodeck:credits-touch-transition
         (retrodeck:credits-initial-state) layout '(1240 40 t t nil))
      (assert (null effect))
      (multiple-value-bind (released release-effect)
          (retrodeck:credits-touch-transition
           pressed layout '(1240 40 nil nil t))
        (assert (null (getf released :pressed-target)))
        (assert (equal release-effect '(:close t :cue :back))))))

  (setf *regular-file-result* nil)
  (assert (signals-error-p
           (lambda () (retrodeck:load-project-credits "/tmp/missing.tsv"))))
  (assert (signals-error-p
           (lambda () (retrodeck:load-project-credits "relative.tsv"))))
  (dolist (contents
           (list "# only a comment\n"
                 "bad\trow\n"
                 "same\trole\tMIT\nsame\trole\tMIT\n"
                 (format nil "~A\trole\tMIT\n" (make-string 49
                                                          :initial-element #\A))
                 (with-output-to-string (output)
                   (dotimes (index 65)
                     (format output "project-~D\trole\tMIT~%" index)))))
    (setf *regular-file-result* contents)
    (assert (signals-error-p
             (lambda ()
               (retrodeck:load-project-credits "/tmp/invalid.tsv"))))))

(let* ((network '(:ssid "net1" :wlan-ipv4 "10.249.110.248"
                  :wireguard-ipv4 "10.0.0.10" :selector "CONNECTED"))
       (*canvas-fill-calls* nil)
       (*canvas-glyph-calls* nil)
       (layout (retrodeck:render-dashboard-settings
                42 60 "us" :volume-down "" network)))
  (assert (= *canvas-clear-color* #x000000))
  (assert (equal layout
                 '(:close (1212 12 56 56)
                   :wifi (926 20 262 108)
                   :volume-down (108 208 104 104)
                   :volume-up (228 208 104 104)
                   :brightness-down (438 208 104 104)
                   :brightness-up (558 208 104 104)
                   :terminal (792 208 112 104)
                   :keymap (1036 208 112 104))))
  (assert (member '(112 208 96 104 #xfe6c27)
                  *canvas-fill-calls* :test #'equal))
  (assert (member '(116 212 88 96 #x503311)
                  *canvas-fill-calls* :test #'equal))
  (assert (member '(232 208 96 104 #x6c6c6c)
                  *canvas-fill-calls* :test #'equal))
  (assert (member '(236 212 88 96 #x303030)
                  *canvas-fill-calls* :test #'equal))
  (assert (member '(64 44 110 3 #xeeeeee)
                  *canvas-glyph-calls* :test #'equal))
  (assert (member '(832 247 62 2 #xeeeeee)
                  *canvas-glyph-calls* :test #'equal))
  (dolist (target '(:close :wifi :volume-down :volume-up
                    :brightness-down :brightness-up :terminal :keymap))
    (destructuring-bind (x y width height) (getf layout target)
      (assert (eq (retrodeck:settings-target-at layout x y) target))
      (assert (eq (retrodeck:settings-target-at
                   layout (+ x width -1) (+ y height -1))
                  target))
      (assert (not (eq (retrodeck:settings-target-at
                        layout (+ x width) y)
                       target)))
      (assert (not (eq (retrodeck:settings-target-at
                        layout x (+ y height))
                       target)))))
  (setf *canvas-fill-calls* nil
        *canvas-glyph-calls* nil)
  (retrodeck:render-dashboard-settings 0 60 "us" :volume-up "" network)
  (assert (member '(194 334 79 3 #xeeeeee)
                  *canvas-glyph-calls* :test #'equal))
  (setf *canvas-glyph-calls* nil)
  (retrodeck:render-dashboard-settings 42 60 "cz" :keymap "" network)
  (assert (member '(1070 246 67 4 #xeeeeee)
                  *canvas-glyph-calls* :test #'equal))
  (setf *canvas-glyph-calls* nil)
  (retrodeck:render-dashboard-settings
   42 100 "us" :brightness-up "BRIGHTNESS 100%" network)
  (assert (member '(551 447 66 2 #xbcbcbc)
                  *canvas-glyph-calls* :test #'equal)))

(assert (string= (retrodeck:dashboard-settings-label :active-wifi)
                 "ACTIVE WIFI"))
(assert (equal (retrodeck:dashboard-settings-geometry :close)
               '(1212 12 56 56)))
(assert (string= (retrodeck:dashboard-settings-path :volume-state)
                 "/mnt/data/nes-deck/state/menu-volume.state"))
(assert (= (retrodeck:settings-volume-after-target :volume-down 42 42) 37))
(assert (= (retrodeck:settings-volume-after-target :volume-down 5 42) 0))
(assert (= (retrodeck:settings-volume-after-target :volume-up 0 42) 42))
(assert (= (retrodeck:settings-volume-after-target :volume-up 0 0) 5))
(assert (= (retrodeck:settings-volume-after-target :volume-up 100 42) 100))
(assert (= (retrodeck:settings-brightness-after-target :brightness-down 10) 10))
(assert (= (retrodeck:settings-brightness-after-target :brightness-down 60) 50))
(assert (= (retrodeck:settings-brightness-after-target :brightness-up 100) 100))

(let ((state (retrodeck:settings-initial-state
              :volume 42 :brightness 60 :keymap "us")))
  (assert (equal state
                 '(:open t :volume 42 :last-audible-volume 42
                   :brightness 60 :keymap "us" :selected :volume-down
                   :pressed-target nil :status "")))
  (multiple-value-bind (moved effect)
      (retrodeck:settings-move-selection state :previous)
    (assert (eq (getf moved :selected) :wifi))
    (assert (equal effect '(:cue :previous)))
    (multiple-value-bind (wrapped next-effect)
        (retrodeck:settings-move-selection moved :next)
      (assert (eq (getf wrapped :selected) :volume-down))
      (assert (equal next-effect '(:cue :next)))))
  (let ((close-selected (copy-list state)))
    (setf (getf close-selected :selected) :close)
    (multiple-value-bind (next effect)
        (retrodeck:settings-move-selection close-selected :next)
      (assert (eq (getf next :selected) :volume-down))
      (assert (equal effect '(:cue :next))))
    (multiple-value-bind (previous effect)
        (retrodeck:settings-move-selection close-selected :previous)
      (assert (eq (getf previous :selected) :wifi))
      (assert (equal effect '(:cue :previous)))))
  (multiple-value-bind (confirmed plan)
      (retrodeck:settings-controller-transition state :confirm)
    (assert (eq (getf confirmed :selected) :volume-down))
    (assert (eq (getf plan :action) :volume))
    (assert (= (getf plan :value) 37))
    (assert (equal (getf plan :success-effect) '(:cue :volume)))
    (assert (string= (getf plan :success-status) "GAME VOLUME 37%"))
    (multiple-value-bind (completed effect)
        (retrodeck:settings-complete-action confirmed plan t)
      (assert (equal effect '(:cue :volume)))
      (assert (= (getf completed :volume) 37))
      (assert (= (getf completed :last-audible-volume) 37))
      (assert (string= (getf completed :status) "GAME VOLUME 37%")))
    (multiple-value-bind (tone-failed effect)
        (retrodeck:settings-complete-action
         confirmed plan t :tone-succeeded-p nil)
      (assert (equal effect '(:cue :volume)))
      (assert (string= (getf tone-failed :status)
                       "VOLUME SAVED; CONFIRMATION TONE FAILED")))
    (multiple-value-bind (failed effect)
        (retrodeck:settings-complete-action confirmed plan nil)
      (assert (null effect))
      (assert (= (getf failed :volume) 42))
      (assert (string= (getf failed :status) "VOLUME STATE ERROR"))))
  (multiple-value-bind (back back-plan)
      (retrodeck:settings-controller-transition state :back)
    (multiple-value-bind (closed effect)
        (retrodeck:settings-complete-action back back-plan t)
      (assert (not (getf closed :open)))
      (assert (equal effect '(:cue :back)))))
  (let* ((muting-state (retrodeck:settings-initial-state
                        :volume 5 :last-audible-volume 42
                        :brightness 60 :keymap "us"))
         (mute-plan (retrodeck:settings-activation-plan
                     muting-state :volume-down)))
    (assert (zerop (getf mute-plan :value)))
    (assert (equal (getf mute-plan :success-effect) '(:stop-sound t)))
    (multiple-value-bind (muted effect)
        (retrodeck:settings-complete-action muting-state mute-plan t)
      (assert (equal effect '(:stop-sound t)))
      (assert (zerop (getf muted :volume)))
      (assert (= (getf muted :last-audible-volume) 42))
      (assert (string= (getf muted :status) "GAME VOLUME MUTED"))
      (let ((restore-plan (retrodeck:settings-activation-plan
                           muted :volume-up)))
        (assert (= (getf restore-plan :value) 42))
        (multiple-value-bind (failed failed-effect)
            (retrodeck:settings-complete-action muted restore-plan nil)
          (assert (null failed-effect))
          (assert (zerop (getf failed :volume)))
          (assert (string= (getf failed :status) "VOLUME STATE ERROR")))))
    (multiple-value-bind (failed effect)
        (retrodeck:settings-complete-action muting-state mute-plan nil)
      (assert (null effect))
      (assert (= (getf failed :volume) 5))
      (assert (string= (getf failed :status) "VOLUME STATE ERROR"))))
  (let ((brightness-plan
          (retrodeck:settings-activation-plan state :brightness-up)))
    (assert (= (getf brightness-plan :value) 70))
    (assert (eq (getf brightness-plan :cue) :next))
    (assert (string= (getf brightness-plan :device-path)
                     "/sys/class/backlight/display-bl/brightness"))
    (multiple-value-bind (brighter effect)
        (retrodeck:settings-complete-action state brightness-plan t)
      (assert (equal effect '(:cue :next)))
      (assert (= (getf brighter :brightness) 70))
      (assert (string= (getf brighter :status) "BRIGHTNESS 70%")))
    (multiple-value-bind (failed effect)
        (retrodeck:settings-complete-action state brightness-plan nil)
      (assert (equal effect '(:cue :next)))
      (assert (= (getf failed :brightness) 60))
      (assert (string= (getf failed :status)
                       "BRIGHTNESS ERROR - CHECK LOG"))))
  (let ((keymap-plan (retrodeck:settings-activation-plan state :keymap)))
    (assert (string= (getf keymap-plan :value) "cz"))
    (assert (eq (getf keymap-plan :cue) :confirm))
    (multiple-value-bind (czech effect)
        (retrodeck:settings-complete-action state keymap-plan t)
      (assert (equal effect '(:cue :confirm)))
      (assert (string= (getf czech :keymap) "cz"))
      (assert (string= (getf czech :status) "TERMINAL KEYS: CZECH"))))
  (assert (equal (retrodeck:settings-activation-plan state :terminal)
                 '(:action :terminal :mode "shell" :cue :confirm)))
  (assert (equal (retrodeck:settings-activation-plan state :wifi)
                 '(:action :wifi :cue :confirm))))

(let* ((layout '(:close (1212 12 56 56)
                 :wifi (926 20 262 108)
                 :volume-down (108 208 104 104)
                 :volume-up (228 208 104 104)
                 :brightness-down (438 208 104 104)
                 :brightness-up (558 208 104 104)
                 :terminal (792 208 112 104)
                 :keymap (1036 208 112 104)))
       (state (retrodeck:settings-initial-state
               :volume 42 :brightness 60 :keymap "us")))
  (multiple-value-bind (pressed effect)
      (retrodeck:settings-touch-transition
       state layout '(128 228 t t nil))
    (assert (null effect))
    (multiple-value-bind (released plan)
        (retrodeck:settings-touch-transition
         pressed layout '(128 228 nil nil t))
      (assert (null (getf released :pressed-target)))
      (assert (eq (getf released :selected) :volume-down))
      (assert (= (getf plan :value) 37))))
  (multiple-value-bind (pressed effect)
      (retrodeck:settings-touch-transition
       state layout '(128 228 t t nil))
    (declare (ignore effect))
    (multiple-value-bind (released plan)
        (retrodeck:settings-touch-transition
         pressed layout '(248 228 nil nil t))
      (assert (null plan))
      (assert (null (getf released :pressed-target)))))
  (multiple-value-bind (pressed effect)
      (retrodeck:settings-touch-transition
       state layout '(1240 40 t t nil))
    (declare (ignore effect))
    (multiple-value-bind (released plan)
        (retrodeck:settings-touch-transition
         pressed layout '(1240 40 nil nil t))
      (assert (eq (getf plan :action) :close))
      (assert (eq (getf plan :cue) :back))
      (assert (eq (getf released :selected) :close)))))

(let* ((network '(:ssid "net1" :wlan-ipv4 "10.249.110.248"
                  :wireguard-ipv4 "10.0.0.10" :selector "CONNECTED"))
       (state (retrodeck:wifi-initial-state))
       (*canvas-fill-calls* nil)
       (*canvas-glyph-calls* nil)
       (layout (retrodeck:render-dashboard-wifi state network)))
  (assert (= *canvas-clear-color* #x000000))
  (assert (equal (subseq layout 0 16)
                 '(:back (16 10 120 62)
                   :ssid (330 10 310 62)
                   :passphrase (650 10 330 62)
                   :save (990 10 274 62)
                   :mode (16 364 152 66)
                   :shift (176 364 168 66)
                   :space (352 364 700 66)
                   :delete (1060 364 204 66))))
  (let ((keys (getf layout :keys)))
    (assert (= (length keys) 30))
    (assert (equal (first keys) '((18 86 119 62) #\q)))
    (assert (equal (nth 10 keys) '((17 154 133 62) #\a)))
    (assert (equal (car (last keys)) '((956 290 307 62) #\-))))
  (assert (member '(330 10 310 62 #x121212)
                  *canvas-fill-calls* :test #'equal))
  (assert (member '(330 10 310 3 #x87afff)
                  *canvas-fill-calls* :test #'equal))
  (assert (member '(650 10 330 3 #x5f5f5f)
                  *canvas-fill-calls* :test #'equal))
  (dolist (target '(:back :ssid :passphrase :save
                    :mode :shift :space :delete))
    (destructuring-bind (x y width height) (getf layout target)
      (assert (eq (retrodeck:wifi-target-at layout x y) target))
      (assert (eq (retrodeck:wifi-target-at
                   layout (+ x width -1) (+ y height -1))
                  target))
      (assert (not (eq (retrodeck:wifi-target-at layout (+ x width) y)
                       target)))
      (assert (not (eq (retrodeck:wifi-target-at layout x (+ y height))
                       target)))))
  (assert (equal (retrodeck:wifi-target-at layout 18 86) '(:key 0 #\q)))
  (assert (equal (retrodeck:wifi-target-at layout 136 147)
                 '(:key 0 #\q)))
  (assert (null (retrodeck:wifi-target-at layout 137 147)))
  (let* ((uppercase (retrodeck:wifi-initial-state :uppercase t))
         (uppercase-layout
           (retrodeck:render-dashboard-wifi uppercase network)))
    (assert (= (length (getf uppercase-layout :keys)) 30))
    (assert (equal (first (getf uppercase-layout :keys))
                   '((18 86 119 62) #\Q))))
  (let* ((symbols (retrodeck:wifi-initial-state
                   :ssid "NETWORK" :passphrase "password"
                   :field :passphrase :symbols t))
         (symbols-layout (retrodeck:render-dashboard-wifi symbols network)))
    (assert (= (length (getf symbols-layout :keys)) 42))
    (assert (equal (nth 30 (getf symbols-layout :keys))
                   '((19 290 98 62) #\`)))
    (assert (equal (car (last (getf symbols-layout :keys)))
                   '((1163 290 98 62) #\>)))))

(assert (string= (retrodeck:dashboard-wifi-label :title) "ADD WIFI"))
(assert (equal (retrodeck:dashboard-wifi-geometry :back) '(16 10 120 62)))
(assert (equal (retrodeck:dashboard-wifi-key-rows :alphabet)
               '("qwertyuiop" "asdfghjkl" "zxcvbnm" "@._-")))
(assert (= (retrodeck:dashboard-wifi-limit :passphrase-maximum) 63))
(assert (string= (retrodeck:dashboard-wifi-path :profile-helper)
                 "/usr/sbin/deck-wifi-profile-add"))
(assert (string= (retrodeck:wifi-tail-for-field "short" 19) "short"))
(assert (string= (retrodeck:wifi-tail-for-field "123456789" 5) "...89"))
(assert (string= (retrodeck:wifi-tail-for-field "123456789" 3) "789"))
(assert (retrodeck:wifi-valid-text-p "test net" 1 32))
(assert (not (retrodeck:wifi-valid-text-p (format nil "bad~%ssid") 1 32)))

(let* ((network '(:ssid "net1" :wlan-ipv4 "10.249.110.248"
                  :wireguard-ipv4 "10.0.0.10" :selector "CONNECTED"))
       (state (retrodeck:wifi-initial-state :status "OLD"))
       (layout (retrodeck:render-dashboard-wifi state network)))
  (assert (equal state
                 '(:open t :ssid "" :passphrase "" :field :ssid
                   :uppercase nil :symbols nil :status "OLD"
                   :pressed-target nil)))
  (let ((opened (retrodeck:wifi-open-state state)))
    (assert (getf opened :open))
    (assert (string= (getf opened :status) "")))
  (multiple-value-bind (keyed applied)
      (retrodeck:wifi-apply-target state layout '(:key 0 #\q))
    (assert applied)
    (assert (string= (getf keyed :ssid) "q"))
    (assert (string= (getf keyed :status) "")))
  (multiple-value-bind (unchanged applied)
      (retrodeck:wifi-apply-target state layout '(:key -1 #\q))
    (assert (not applied))
    (assert (equal unchanged state)))
  (multiple-value-bind (password-field applied)
      (retrodeck:wifi-apply-target state layout :passphrase)
    (assert applied)
    (assert (eq (getf password-field :field) :passphrase))
    (multiple-value-bind (spaced space-applied)
        (retrodeck:wifi-apply-target password-field layout :space)
      (assert space-applied)
      (assert (string= (getf spaced :passphrase) " "))
      (multiple-value-bind (deleted delete-applied)
          (retrodeck:wifi-apply-target spaced layout :delete)
        (assert delete-applied)
        (assert (string= (getf deleted :passphrase) "")))))
  (multiple-value-bind (symbols applied)
      (retrodeck:wifi-apply-target state layout :mode)
    (assert applied)
    (assert (getf symbols :symbols))
    (multiple-value-bind (unchanged shift-applied)
        (retrodeck:wifi-apply-target symbols layout :shift)
      (assert (not shift-applied))
      (assert (not (getf unchanged :uppercase)))
      (assert (string= (getf unchanged :status) ""))))
  (multiple-value-bind (uppercase applied)
      (retrodeck:wifi-apply-target state layout :shift)
    (assert applied)
    (assert (getf uppercase :uppercase)))
  (let ((full (retrodeck:wifi-initial-state
               :ssid (make-string 32 :initial-element #\x) :status "OLD")))
    (multiple-value-bind (limited applied)
        (retrodeck:wifi-apply-target full layout '(:key 0 #\q))
      (assert applied)
      (assert (= (length (getf limited :ssid)) 32))
      (assert (string= (getf limited :status) ""))))
  (multiple-value-bind (ignored effect)
      (retrodeck:wifi-controller-transition state :confirm)
    (assert (equal ignored state))
    (assert (null effect)))
  (multiple-value-bind (closed effect)
      (retrodeck:wifi-controller-transition state :back)
    (assert (not (getf closed :open)))
    (assert (equal effect
                   '(:action :close :dashboard-status "WIFI EDITOR CLOSED"
                     :cue :back))))
  (multiple-value-bind (invalid effect)
      (retrodeck:wifi-activate-target state layout :save)
    (assert (string= (getf invalid :status)
                     "SSID MUST BE 1 TO 32 CHARACTERS"))
    (assert (equal effect '(:cue :confirm))))
  (let ((short-password
          (retrodeck:wifi-initial-state :ssid "test net" :passphrase "short")))
    (multiple-value-bind (invalid effect)
        (retrodeck:wifi-activate-target short-password layout :save)
      (assert (string= (getf invalid :status)
                       "PASSWORD MUST BE 8 TO 63 CHARACTERS"))
      (assert (equal effect '(:cue :confirm)))))
  (let ((valid (retrodeck:wifi-initial-state
                :ssid "test net" :passphrase "secret!9")))
    (multiple-value-bind (plan error-status) (retrodeck:wifi-save-plan valid)
      (assert (null error-status))
      (assert (eq (getf plan :action) :save))
      (assert (string= (getf plan :executable)
                       "/usr/sbin/deck-wifi-profile-add"))
      (assert (string= (getf plan :input) "test net
secret!9
"))
      (multiple-value-bind (saved effect)
          (retrodeck:wifi-complete-save valid plan t)
        (assert (string= (getf saved :passphrase) ""))
        (assert (string= (getf saved :status)
                         "WIFI SAVED - USED AFTER CURRENT WIFI DISCONNECTS"))
        (assert (equal effect '(:cue :confirm))))
      (multiple-value-bind (failed effect)
          (retrodeck:wifi-complete-save
           valid plan nil :failure-status "WIFI PROFILE WRITE FAILED")
        (assert (string= (getf failed :passphrase) "secret!9"))
        (assert (string= (getf failed :status)
                         "WIFI PROFILE WRITE FAILED"))
        (assert (equal effect '(:cue :confirm)))))))

(let* ((network '(:ssid "net1" :wlan-ipv4 "10.249.110.248"
                  :wireguard-ipv4 "10.0.0.10" :selector "CONNECTED"))
       (state (retrodeck:wifi-initial-state
               :ssid "test net" :passphrase "secret!9"))
       (layout (retrodeck:render-dashboard-wifi state network)))
  (multiple-value-bind (pressed effect)
      (retrodeck:wifi-touch-transition state layout '(18 86 t t nil))
    (assert (null effect))
    (multiple-value-bind (released release-effect)
        (retrodeck:wifi-touch-transition pressed layout '(18 86 nil nil t))
      (assert (string= (getf released :ssid) "test netq"))
      (assert (equal release-effect '(:cue :next)))))
  (multiple-value-bind (pressed effect)
      (retrodeck:wifi-touch-transition state layout '(18 86 t t nil))
    (declare (ignore effect))
    (multiple-value-bind (released release-effect)
        (retrodeck:wifi-touch-transition pressed layout '(143 86 nil nil t))
      (assert (string= (getf released :ssid) "test net"))
      (assert (null release-effect))))
  (multiple-value-bind (pressed effect)
      (retrodeck:wifi-touch-transition state layout '(1000 20 t t nil))
    (declare (ignore effect))
    (multiple-value-bind (released plan)
        (retrodeck:wifi-touch-transition pressed layout '(1000 20 nil nil t))
      (assert (eq (getf plan :action) :save))
      (assert (string= (getf released :passphrase) "secret!9"))))
  (multiple-value-bind (pressed effect)
      (retrodeck:wifi-touch-transition state layout '(20 20 t t nil))
    (declare (ignore effect))
    (multiple-value-bind (released plan)
        (retrodeck:wifi-touch-transition pressed layout '(20 20 nil nil t))
      (assert (not (getf released :open)))
      (assert (eq (getf plan :action) :close)))))

(let* ((games '((:id "alpha" :title "ALPHA" :system :nes :color #x5f87ff)
                (:id "beta" :title "BETA" :system :nes :color #xafd75f)
                (:id "long-title" :title "A VERY LONG FIXTURE GAME TITLE"
                 :system :nes :color #xffd700)
                (:id "delta" :title "DELTA" :system :nes :color #xd75f5f)
                (:id "gb" :title "GB FIXTURE" :system :gb :color #x87af87)
                (:id "gbc" :title "GBC FIXTURE" :system :gbc :color #xecb6e7)
                (:id "zx" :title "ZX FIXTURE" :system :zx :color #x87afff)
                (:id "chip8" :title "CHIP-8 FIXTURE" :system :chip8
                 :color #xffffaf)
                (:id "deck-fixture" :title "DECK FIXTURE" :system :deck
                 :color #xff8700)))
       (*canvas-fill-calls* nil)
       (*canvas-glyph-calls* nil)
       (layout (retrodeck:render-dashboard games :nes 2 "FIXTURE STATUS")))
  (assert (= *canvas-clear-color* #x000000))
  (assert (equal (getf layout :systems) '(:nes :gb :gbc :zx :chip8 :deck)))
  (assert (equal (getf layout :system-buttons)
                 '((56 76 188 52) (252 76 188 52) (448 76 188 52)
                   (644 76 188 52) (840 76 188 52) (1036 76 188 52))))
  (assert (equal (getf layout :game-indices) '(0 1 2 3)))
  (assert (= (getf layout :shown-game-index) 2))
  (assert (equal (getf layout :visible-game-indices) '(1 2 3)))
  (assert (equal (getf layout :game-buttons)
                 '((280 154 216 264) (532 154 216 264) (784 154 216 264))))
  (assert (equal (getf layout :indicators)
                 '((596 438 16 8) (620 438 16 8)
                   (644 438 16 8) (668 438 16 8))))
  (assert (equal (getf layout :previous) '(156 232 80 100)))
  (assert (equal (getf layout :next) '(1044 232 80 100)))
  (assert (member '(536 154 208 264 #xfe6c27) *canvas-fill-calls*
                  :test #'equal))
  (assert (member '(536 162 208 248 #x503311) *canvas-fill-calls*
                  :test #'equal))
  (assert (member '(557 457 70 2 #xbcbcbc) *canvas-glyph-calls*
                  :test #'equal)))

(let* ((games '((:id "only-nes" :title "ONLY NES" :system :nes
                 :color #x5f87ff)))
       (layout (retrodeck:render-dashboard games :gb 0 "NO MATCH")))
  (assert (= (getf layout :shown-game-index) (length games)))
  (assert (null (getf layout :game-indices)))
  (assert (null (getf layout :visible-game-indices)))
  (assert (null (getf layout :game-buttons)))
  (assert (null (getf layout :indicators)))
  (assert (equal (getf layout :previous) '(156 232 80 100)))
  (assert (equal (getf layout :next) '(1044 232 80 100))))

(let* ((games '((:id "nes" :title "NES" :system :nes :color #x5f87ff)
                (:id "gb" :title "ONLY GB" :system :gb :color #x87af87)))
       (layout (retrodeck:render-dashboard games :gb 7 "ONE CARD")))
  (assert (= (getf layout :shown-game-index) 1))
  (assert (equal (getf layout :game-indices) '(1)))
  (assert (equal (getf layout :visible-game-indices) '(1)))
  (assert (equal (getf layout :game-buttons) '((532 154 216 264))))
  (assert (equal (getf layout :indicators) '((632 438 16 8))))
  (assert (equal (getf layout :previous) '(0 0 0 0)))
  (assert (equal (getf layout :next) '(0 0 0 0))))

(assert (retrodeck:clear-dashboard-raster-cache))
(assert (= *raster-clear-count* 1))
(setf *raster-png-result* 23
      *raster-png-calls* nil
      *canvas-raster-calls* nil)
(let ((retrodeck:*dashboard-settings-icon-path* "/tmp/settings.png"))
  (retrodeck:render-dashboard nil :nes 0 "")
  (assert (equal *raster-png-arguments* '("/tmp/settings.png" 23 23)))
  (assert (= (length *raster-png-calls*) 1))
  (assert (member '(23 1215 415 50 50) *canvas-raster-calls*
                  :test #'equal)))

(assert (retrodeck:clear-dashboard-raster-cache))
(assert (= *raster-clear-count* 2))
(setf *raster-png-result* 0
      *raster-cover-result* 24
      *raster-cover-calls* nil
      *canvas-fill-calls* nil
      *canvas-raster-calls* nil)
(let* ((games '((:id "covered" :title "COVERED" :system :nes
                 :color #x5f87ff :cover "/tmp/fixture.png")))
       (layout (retrodeck:render-dashboard games :nes 0 "COVERED")))
  (assert (= (getf layout :shown-game-index) 0))
  (assert (equal *raster-cover-arguments*
                 '("/tmp/fixture.png" #x5f87ff)))
  (assert (= (length *raster-cover-calls*) 1))
  (assert (member '(24 540 162 200 200) *canvas-raster-calls*
                  :test #'equal))
  (assert (not (member '(578 190 124 144 #x5f87ff) *canvas-fill-calls*
                       :test #'equal))))
(setf *raster-cover-result* 0)

(setf *evdev-controls-scan-result* '(2 3)
      *evdev-controls-dispatch-result* '(2 1)
      *evdev-controls* '((0 15 1)))
(assert (equal (retrodeck:scan-evdev-controls)
               '(:gamepads 2 :keyboards 3)))
(assert (equal (retrodeck:dispatch-evdev-controls 25)
               '(:count 2 :rescan t)))
(assert (= *evdev-controls-dispatch-timeout* 25))
(assert (equal (retrodeck:next-evdev-control)
               '(:kind :keyboard :code 15 :shift t :repeat nil)))
(setf *evdev-controls* '((1 #x501 0)))
(assert (equal (retrodeck:next-evdev-control)
               '(:kind :gamepad :edges #x501)))
(setf *evdev-controls* nil)
(assert (null (retrodeck:next-evdev-control)))
(assert (retrodeck:close-evdev-controls))
(assert (= *evdev-controls-close-count* 1))
(setf *evdev-controls-scan-result* '(3 0))
(assert (signals-error-p #'retrodeck:scan-evdev-controls))
(setf *evdev-controls-scan-result* nil
      *evdev-controls-dispatch-result* nil)
(assert (null (retrodeck:scan-evdev-controls)))
(assert (null (retrodeck:dispatch-evdev-controls)))
(assert (signals-type-error-p
         (lambda () (retrodeck:dispatch-evdev-controls #x100000000))))

(assert (equal retrodeck:*dashboard-keyboard-controls*
               '((1 :back)
                 (15 :system-next :system-previous)
                 (28 :confirm)
                 (96 :confirm)
                 (103 :up)
                 (105 :left)
                 (106 :right)
                 (108 :down))))
(assert (equal retrodeck:*dashboard-gamepad-controls*
               '((#x001 . :back)
                 (#x002 . :back)
                 (#x004 . :confirm)
                 (#x008 . :confirm)
                 (#x010 . :system-previous)
                 (#x020 . :system-next)
                 (#x040 . :settings)
                 (#x100 . :left)
                 (#x200 . :right)
                 (#x400 . :up)
                 (#x800 . :down))))
(dolist (fixture '((1 nil :back)
                   (15 nil :system-next)
                   (15 t :system-previous)
                   (28 nil :confirm)
                   (96 nil :confirm)
                   (103 nil :up)
                   (105 nil :left)
                   (106 nil :right)
                   (108 nil :down)))
  (destructuring-bind (code shift expected) fixture
    (assert (equal (retrodeck:dashboard-control-actions
                    (list :kind :keyboard :code code
                          :shift shift :repeat nil))
                   (list expected)))))
(dolist (definition retrodeck:*dashboard-gamepad-controls*)
  (assert (equal (retrodeck:dashboard-control-actions
                  (list :kind :gamepad :edges (car definition)))
                 (list (cdr definition)))))
(assert (null (retrodeck:dashboard-control-actions
               '(:kind :gamepad :edges #x080))))
(assert (equal (retrodeck:dashboard-control-actions
                '(:kind :keyboard :code 28 :shift nil :repeat nil))
               '(:confirm)))
(assert (equal (retrodeck:dashboard-control-actions
                '(:kind :keyboard :code 15 :shift nil :repeat nil))
               '(:system-next)))
(assert (equal (retrodeck:dashboard-control-actions
                '(:kind :keyboard :code 15 :shift t :repeat nil))
               '(:system-previous)))
(assert (null (retrodeck:dashboard-control-actions
               '(:kind :keyboard :code 30 :shift nil :repeat nil))))
(assert (equal (retrodeck:dashboard-control-actions
                '(:kind :gamepad :edges #x905))
               '(:back :confirm :left :down)))
(setf *evdev-controls* '((0 106 2) (0 15 1) (1 #x224 0)))
(multiple-value-bind (gamepad keyboard)
    (retrodeck:collect-dashboard-control-actions)
  (assert (and (subsetp gamepad '(:confirm :system-next :right))
               (subsetp '(:confirm :system-next :right) gamepad)))
  (assert (and (subsetp keyboard '(:right :system-previous))
               (subsetp '(:right :system-previous) keyboard))))
(assert (null *evdev-controls*))

(assert (null (retrodeck:dashboard-controller-command '(:back) nil nil)))
(assert (eq (retrodeck:dashboard-controller-command
             '(:back :settings) t nil)
            :back))
(assert (eq (retrodeck:dashboard-controller-command
             '(:back :confirm) nil t)
            :back))
(assert (null (retrodeck:dashboard-controller-command
               '(:settings :confirm) t nil)))
(assert (eq (retrodeck:dashboard-controller-command
             '(:settings :system-previous :confirm) nil nil)
            :settings))
(assert (eq (retrodeck:dashboard-controller-command
             '(:system-previous :system-next) nil nil)
            :system-previous))
(assert (null (retrodeck:dashboard-controller-command
               '(:system-next) nil t)))
(assert (eq (retrodeck:dashboard-controller-command
             '(:left :right :confirm) nil nil)
            :previous))
(assert (eq (retrodeck:dashboard-controller-command
             '(:down :confirm) nil nil)
            :next))
(assert (eq (retrodeck:dashboard-controller-command
             '(:confirm) nil nil)
            :confirm))

(assert (retrodeck:dashboard-controller-scan-due-p nil 0))
(assert (retrodeck:dashboard-controller-scan-due-p 0 999))
(assert (not (retrodeck:dashboard-controller-scan-due-p 1 999)))
(assert (retrodeck:dashboard-controller-scan-due-p 1 1001))
(assert (retrodeck:dashboard-controller-scan-due-p 900 901 :force t))
(assert (retrodeck:dashboard-controller-scan-due-p 900 901 :rescan t))

(let ((guard (retrodeck:dashboard-controller-guard-initial-state)))
  (dotimes (index 12)
    (multiple-value-bind (next accepted suspended)
        (retrodeck:dashboard-controller-guard-accept-edge guard (* index 50))
      (assert accepted)
      (assert (not suspended))
      (setf guard next)))
  (multiple-value-bind (next accepted suspended)
      (retrodeck:dashboard-controller-guard-accept-edge guard 600)
    (assert (not accepted))
    (assert suspended)
    (assert (getf next :suspended))
    (setf guard next))
  (multiple-value-bind (next accepted suspended)
      (retrodeck:dashboard-controller-guard-accept-edge guard 650)
    (assert (not accepted))
    (assert (not suspended))
    (assert (= (getf next :last-edge-at) 650))
    (setf guard next))
  (multiple-value-bind (next recovered)
      (retrodeck:dashboard-controller-guard-recover-if-quiet guard 1649)
    (assert (not recovered))
    (assert (getf next :suspended)))
  (multiple-value-bind (next recovered)
      (retrodeck:dashboard-controller-guard-recover-if-quiet guard 1650)
    (assert recovered)
    (assert (equal next
                   '(:edge-times nil :suspended nil :last-edge-at nil)))))

(let ((guard (retrodeck:dashboard-controller-guard-initial-state)))
  (multiple-value-bind (next accepted suspended)
      (retrodeck:dashboard-controller-guard-accept-edge guard 0)
    (declare (ignore suspended))
    (assert accepted)
    (setf guard next))
  (multiple-value-bind (next accepted suspended)
      (retrodeck:dashboard-controller-guard-accept-edge guard 1000)
    (declare (ignore suspended))
    (assert accepted)
    (assert (equal (getf next :edge-times) '(1000)))))

(setf *active-status* 0
      retrodeck::*menu-sound-input-until-ms* 0)
(let ((guard (retrodeck:dashboard-controller-guard-initial-state)))
  (multiple-value-bind (actions next suspended)
      (retrodeck:dashboard-controller-input-actions
       '(:confirm) '(:right) guard 100)
    (assert (and (member :confirm actions) (member :right actions)))
    (assert (not suspended))
    (assert (equal (getf next :edge-times) '(100)))
    (setf guard next))
  (setf *active-status* 1)
  (multiple-value-bind (actions next suspended)
      (retrodeck:dashboard-controller-input-actions
       '(:confirm) '(:right) guard 150)
    (assert (equal actions '(:right)))
    (assert (not suspended))
    (assert (equal (getf next :edge-times) '(100 150)))))
(setf *active-status* 0)

(setf *evdev-touch* '(17 23 1 1 0)
      *evdev-dispatch-result* 3)
(assert (retrodeck:open-evdev-touch))
(assert (= (retrodeck:dispatch-evdev-touch 25) 3))
(assert (= *evdev-dispatch-timeout* 25))
(assert (equal (retrodeck:next-evdev-touch) '(17 23 t t nil)))
(setf *evdev-touch* nil
      *evdev-dispatch-result* -1)
(assert (null (retrodeck:next-evdev-touch)))
(assert (null (retrodeck:dispatch-evdev-touch)))
(assert (retrodeck:close-evdev-touch))
(assert (= *evdev-close-count* 1))

(setf *fbdev-size* '(1280 480))
(assert (retrodeck:open-fbdev))
(assert (equal (retrodeck:current-fbdev-size) '(1280 480)))
(assert (retrodeck:present-fbdev-canvas))
(assert (retrodeck:present-fbdev-solid #xfe6c27))
(assert (= *fbdev-present-color* #xfe6c27))
(assert (retrodeck:close-fbdev))
(assert (= *fbdev-close-count* 1))

(assert (retrodeck:open-wayland-widget))
(assert (retrodeck:close-wayland))
(assert (= *wayland-close-count* 1))
(assert (retrodeck:present-wayland-canvas))
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

(assert (equal retrodeck:*dashboard-systems*
               '((:nes "NES")
                 (:gb "GAME BOY")
                 (:gbc "GBC")
                 (:zx "ZX SPECTRUM")
                 (:chip8 "CHIP-8")
                 (:deck "DECK"))))
(assert (string= (retrodeck:dashboard-system-label :gbc) "GBC"))
(assert (string= (retrodeck:dashboard-system-label :other) "other"))
(assert (string= (retrodeck:dashboard-system-label "MiXeD") "MiXeD"))
(assert (equal retrodeck:*dashboard-palette*
               '((:background . #x000000)
                 (:text-dark . #x121212)
                 (:field . #x121212)
                 (:surface . #x1c1c1c)
                 (:inactive-border . #x5f5f5f)
                 (:control-border . #x6c6c6c)
                 (:footer . #xbcbcbc)
                 (:inactive-text . #xdadada)
                 (:text . #xeeeeee)
                 (:white . #xffffff)
                 (:title . #xffffaf)
                 (:volume-off . #xaf8787)
                 (:volume-on . #x87af87)
                 (:selected . #xecb6e7)
                 (:wifi-active . #x5f87af)
                 (:wifi-focus . #x87afff)
                 (:wifi-active-border . #xafafff)
                 (:field-label . #xafafaf)
                 (:accent . #xfe6c27)
                 (:active . #x503311)
                 (:control-surface . #x303030)
                 (:muted . #x949494))))
(assert (= (retrodeck:dashboard-color :accent) #xfe6c27))
(assert (equal retrodeck:*dashboard-executables*
               '((:nes . "/mnt/data/nes-deck/nes-deck")
                 (:gb . "/mnt/data/nes-deck/gb-deck")
                 (:zx . "/mnt/data/nes-deck/zx-deck")
                 (:chip8 . "/mnt/data/nes-deck/chip8-deck")
                 (:deck . "/mnt/data/nes-deck/ten-seconds-deck")
                 (:chiptunes . "/mnt/data/nes-deck/chiptune-deck")
                 (:terminal . "/mnt/data/nes-deck/terminal/retro-terminal")
                 (:reboot . "/sbin/reboot"))))
(assert (string= retrodeck:*dashboard-cover-directory*
                 "/mnt/data/nes-deck/covers/"))
(assert (string= retrodeck:*dashboard-settings-icon-path*
                 "/mnt/data/nes-deck/menu/settings-icon.png"))
(assert (equal retrodeck:*dashboard-timings*
               '((:child-touch-exit-ms . 2000)
                 (:child-term-grace-ms . 4000)
                 (:reboot-confirm-ms . 4000)
                 (:controller-burst-window-ms . 1000)
                 (:controller-quiet-reset-ms . 1000)
                 (:controller-scan-ms . 1000)
                 (:main-poll-ms . 250)
                 (:animated-poll-ms . 40)
                 (:network-refresh-ms . 2000)
                 (:console-mirror-ms . 100))))
(assert (= (retrodeck:dashboard-timing :reboot-confirm-ms) 4000))
(assert (= retrodeck:*dashboard-volume-default* 42))
(assert (= retrodeck:*dashboard-volume-step* 5))
(assert (= retrodeck:*dashboard-brightness-minimum* 10))
(assert (= retrodeck:*dashboard-brightness-step* 10))
(assert (= retrodeck:*dashboard-controller-burst-limit* 12))
(assert (string= retrodeck:*dashboard-reboot-confirmation-text*
                 "PRESS A OR TAP AGAIN TO REBOOT"))
(assert (string= retrodeck:*dashboard-terminal-login-shell* "/BIN/ASH"))

(assert (equal retrodeck:*dashboard-built-in-applications*
               '((:id "lua-repl"
                  :title "LUA REPL"
                  :system :deck
                  :rom "/mnt/data/nes-deck/terminal/retro-terminal"
                  :color #x5f87ff
                  :terminal-mode "lua")
                 (:id "lisp-repl"
                  :title "LISP REPL"
                  :system :deck
                  :rom "/mnt/data/nes-deck/terminal/retro-terminal"
                  :color #xafd75f
                  :terminal-mode "lisp")
                 (:id "python-repl"
                  :title "PYTHON REPL"
                  :system :deck
                  :rom "/mnt/data/nes-deck/terminal/retro-terminal"
                  :color #xffd700
                  :terminal-mode "python")
                 (:id "scheme-repl"
                  :title "SCHEME REPL"
                  :system :deck
                  :rom "/mnt/data/nes-deck/terminal/retro-terminal"
                  :color #x87d787
                  :terminal-mode "scheme")
                 (:id "chiptunes"
                  :title "CHIPTUNES"
                  :system :deck
                  :rom "/mnt/data/chiptunes"
                  :color #xff8700)
                 (:id "terminal"
                  :title "TERMINAL"
                  :system :deck
                  :rom "/mnt/data/nes-deck/terminal/retro-terminal"
                  :color #x5f87af
                  :terminal-mode "shell")
                 (:id "reboot"
                  :title "REBOOT"
                  :system :deck
                  :rom "/sbin/reboot"
                  :color #xd75f5f))))
(assert (null (retrodeck:dashboard-application "missing")))
(let ((application (retrodeck:dashboard-application "terminal")))
  (setf (getf application :title) "CHANGED")
  (assert (string= (getf (retrodeck:dashboard-application "terminal") :title)
                   "TERMINAL")))

(let ((plan
        (retrodeck:dashboard-launch-plan
         '(:id "mario" :title "SUPER MARIO BROS." :system :nes
           :rom "/mnt/data/roms/nes/super-mario-bros.nes" :color #xd78787)
         42 :wayland t
         :volume-state "/mnt/data/nes-deck/state/menu-volume.state")))
  (assert (equal plan
                 '(:executable "/mnt/data/nes-deck/nes-deck"
                   :arguments ("/mnt/data/roms/nes/super-mario-bros.nes")
                   :environment
                   (("RETRO_DECK_VOLUME_PERCENT" . "42")
                    ("RETRO_DECK_EXIT_HINT" . "1")
                    ("RETRO_DECK_PRESENTATION" . "layer-shell")
                    ("RETRO_DECK_VOLUME_STATE" .
                     "/mnt/data/nes-deck/state/menu-volume.state"))
                   :label "mario"
                   :touch-supervision t
                   :mirror-console nil))))

(let ((plan
        (retrodeck:dashboard-launch-plan
         '(:id "zelda-oracle" :title "ZELDA ORACLE" :system :gbc
           :rom "/mnt/data/roms/gbc/zelda-oracle.gbc" :color #x87d787)
         55)))
  (assert (equal plan
                 '(:executable "/mnt/data/nes-deck/gb-deck"
                   :arguments ("/mnt/data/roms/gbc/zelda-oracle.gbc")
                   :environment
                   (("RETRO_DECK_VOLUME_PERCENT" . "55")
                    ("RETRO_DECK_EXIT_HINT" . "1"))
                   :label "zelda-oracle"
                   :touch-supervision t
                   :mirror-console nil))))

(let ((plan
        (retrodeck:dashboard-launch-plan
         '(:id "ten-seconds" :title "10 SECONDS" :system :deck
           :rom "/mnt/data/nes-deck/games/ten-seconds" :color #xffaf87)
         17 :wayland t)))
  (assert (equal plan
                 '(:executable "/mnt/data/nes-deck/ten-seconds-deck"
                   :arguments nil
                   :environment
                   (("RETRO_DECK_VOLUME_PERCENT" . "17")
                    ("RETRO_DECK_PRESENTATION" . "layer-shell"))
                   :label "ten-seconds"
                   :touch-supervision nil
                   :mirror-console nil))))

(let ((plan
        (retrodeck:dashboard-launch-plan
         (retrodeck:dashboard-application "chiptunes") 63)))
  (assert (equal plan
                 '(:executable "/mnt/data/nes-deck/chiptune-deck"
                   :arguments ("/mnt/data/chiptunes")
                   :environment (("RETRO_DECK_VOLUME_PERCENT" . "63"))
                   :label "chiptunes"
                   :touch-supervision nil
                   :mirror-console nil))))

(let ((plan
        (retrodeck:dashboard-launch-plan
         (retrodeck:dashboard-application "lisp-repl") 42 :keymap "cz")))
  (assert (equal plan
                 '(:executable
                   "/mnt/data/nes-deck/terminal/retro-terminal"
                   :arguments ("lisp")
                   :environment (("RETRO_DECK_KEYMAP" . "cz"))
                   :label "lisp REPL"
                   :touch-supervision t
                   :mirror-console t))))

(let* ((plan (retrodeck:dashboard-launch-plan
              (retrodeck:dashboard-application "terminal") 42 :keymap "cz"))
       (fixtures '(((0 0 -1 -1 "exec failed")
                    "TERMINAL ERROR - CHECK LOG")
                   ((0 0 -1 -1 nil) "TERMINAL DID NOT START")
                   ((1 1 -1 15 nil) "RETURNED FROM TERMINAL")
                   ((1 0 0 -1 nil) "TERMINAL EXITED")
                   ((1 0 7 -1 nil) "TERMINAL EXITED (STATUS 7)")
                   ((1 0 -1 15 nil) "TERMINAL STOPPED (SIGNAL 15)")
                   ((1 0 -1 -1 nil) "TERMINAL STOPPED"))))
  (assert (string= (retrodeck:dashboard-terminal-title plan) "TERMINAL"))
  (assert (string= (retrodeck:dashboard-terminal-starting-status plan)
                   "STARTING TERMINAL"))
  (dolist (fixture fixtures)
    (destructuring-bind (native-result expected) fixture
      (assert
       (string=
        (retrodeck:dashboard-terminal-result-status
         plan (retrodeck::decode-native-terminal-result native-result))
        expected))))
  (let ((repl-plan (retrodeck:dashboard-launch-plan
                    (retrodeck:dashboard-application "lisp-repl")
                    42 :keymap "cz")))
    (assert (string= (retrodeck:dashboard-terminal-title repl-plan)
                     "LISP REPL"))
    (assert (string= (retrodeck:dashboard-terminal-starting-status repl-plan)
                     "STARTING LISP REPL"))))

(dolist (result '((2 0 -1 -1 nil)
                  (1 -1 -1 -1 nil)
                  (1 0 -2 -1 nil)
                  (1 0 -1 0 nil)
                  (0 1 -1 -1 nil)
                  (0 0 0 -1 nil)
                  (1 0 0 15 nil)))
  (assert (signals-error-p
           (lambda () (retrodeck::decode-native-terminal-result result)))))

(let* ((plan (retrodeck:dashboard-launch-plan
              (retrodeck:dashboard-application "terminal") 42 :keymap "cz"))
       (before *finish-count*))
  (setf *terminal-result* '(1 0 0 -1 nil)
        *terminal-arguments* nil
        retrodeck::*menu-sound-input-until-ms* 100)
  (assert (equal (retrodeck:run-dashboard-terminal plan)
                 '(:started t :exited-for-touch nil
                   :exit-code 0 :signal nil :error nil)))
  (assert (= *finish-count* (1+ before)))
  (assert (= retrodeck::*menu-sound-input-until-ms* 0))
  (assert (equal *terminal-arguments*
                 '("/mnt/data/nes-deck/terminal/retro-terminal"
                   "cz" "shell" "terminal"))))

(let ((plan
        (retrodeck:dashboard-launch-plan
         (retrodeck:dashboard-application "reboot") 42)))
  (assert (equal plan
                 '(:executable "/sbin/reboot"
                   :arguments nil
                   :environment nil
                   :label "reboot"
                   :touch-supervision t
                   :mirror-console nil))))

(assert (retrodeck:reboot-confirmation-active-p 5000 4999))
(assert (not (retrodeck:reboot-confirmation-active-p 5000 5000)))
(assert (not (retrodeck:reboot-confirmation-active-p 0 0)))

(let* ((games '((:id "alpha" :title "ALPHA" :system :nes :color #x5f87ff)
                (:id "beta" :title "BETA" :system :nes :color #xafd75f)
                (:id "gamma" :title "GAMMA" :system :nes :color #xffffaf)
                (:id "delta" :title "DELTA" :system :nes :color #xd75f5f)
                (:id "gb" :title "GB" :system :gb :color #x87af87)))
       (layout (retrodeck:render-dashboard games :nes 0 "STALE"))
       (state (retrodeck:dashboard-initial-state games)))
  (assert (equal state
                 '(:active-system :nes :game-position 0
                   :pressed-target nil :status "")))
  (assert (equal (retrodeck:dashboard-initial-state
                  '((:id "other" :system :other)))
                 '(:active-system :other :game-position 0
                   :pressed-target nil :status "")))
  (assert (equal (retrodeck:dashboard-initial-state nil)
                 '(:active-system nil :game-position 0
                   :pressed-target nil :status "")))
  (assert (eq (retrodeck:dashboard-target-at layout 12 412) :credits))
  (assert (eq (retrodeck:dashboard-target-at layout 1212 412) :settings))
  (assert (eq (retrodeck:dashboard-target-at layout 157 233) :previous))
  (assert (eq (retrodeck:dashboard-target-at layout 1045 233) :next))
  (assert (equal (retrodeck:dashboard-target-at layout 56 76)
                 '(:system :nes)))
  (assert (equal (retrodeck:dashboard-target-at layout 934 102)
                 '(:system :gb)))
  (assert (equal (retrodeck:dashboard-target-at layout 388 286)
                 '(:game 0)))
  (assert (null (retrodeck:dashboard-target-at layout 636 100)))
  (assert (null (retrodeck:dashboard-target-at layout 68 412)))

  (setf (getf state :status) "STALE")
  (multiple-value-bind (pressed effect)
      (retrodeck:dashboard-touch-transition state layout
                                            '(1084 282 t t nil))
    (assert (eq (getf pressed :pressed-target) :next))
    (assert (null effect))
    (assert (null (getf state :pressed-target)))
    (multiple-value-bind (released release-effect)
        (retrodeck:dashboard-touch-transition pressed layout
                                              '(1084 282 nil nil t))
      (assert (= (getf released :game-position) 1))
      (assert (string= (getf released :status) ""))
      (assert (null (getf released :pressed-target)))
      (assert (equal release-effect '(:render t :cue :next)))))

  (multiple-value-bind (pressed effect)
      (retrodeck:dashboard-touch-transition state layout
                                            '(196 282 t t nil))
    (assert (null effect))
    (multiple-value-bind (released release-effect)
        (retrodeck:dashboard-touch-transition pressed layout
                                              '(196 282 nil nil t))
      (assert (= (getf released :game-position) 3))
      (assert (equal release-effect '(:render t :cue :previous)))))

  (let ((positioned (copy-list state)))
    (setf (getf positioned :game-position) 3)
    (multiple-value-bind (pressed effect)
        (retrodeck:dashboard-touch-transition positioned layout
                                              '(346 102 t t nil))
      (assert (null effect))
      (multiple-value-bind (released release-effect)
          (retrodeck:dashboard-touch-transition pressed layout
                                                '(346 102 nil nil t))
        (assert (eq (getf released :active-system) :nes))
        (assert (zerop (getf released :game-position)))
        (assert (equal release-effect '(:render t))))))

  (multiple-value-bind (pressed effect)
      (retrodeck:dashboard-touch-transition state layout
                                            '(934 102 t t nil))
    (assert (null effect))
    (multiple-value-bind (released release-effect)
        (retrodeck:dashboard-touch-transition pressed layout
                                              '(934 102 nil nil t))
      (assert (eq (getf released :active-system) :gb))
      (assert (zerop (getf released :game-position)))
      (assert (equal release-effect '(:render t :cue :next)))))

  (multiple-value-bind (pressed effect)
      (retrodeck:dashboard-touch-transition state layout
                                            '(1084 282 t t nil))
    (assert (null effect))
    (multiple-value-bind (released release-effect)
        (retrodeck:dashboard-touch-transition pressed layout
                                              '(196 282 nil nil t))
      (assert (zerop (getf released :game-position)))
      (assert (null (getf released :pressed-target)))
      (assert (null release-effect))))

  (multiple-value-bind (pressed effect)
      (retrodeck:dashboard-touch-transition state layout
                                            '(1084 282 t t nil))
    (assert (null effect))
    (multiple-value-bind (released release-effect)
        (retrodeck:dashboard-touch-transition pressed layout
                                              '(-1 -1 nil nil t))
      (assert (zerop (getf released :game-position)))
      (assert (null (getf released :pressed-target)))
      (assert (null release-effect)))))

(let* ((games '((:id "alpha" :title "ALPHA" :system :nes :color #x5f87ff)
                (:id "beta" :title "BETA" :system :nes :color #xafd75f)
                (:id "gamma" :title "GAMMA" :system :nes :color #xffffaf)
                (:id "delta" :title "DELTA" :system :nes :color #xd75f5f)))
       (layout (retrodeck:render-dashboard games :nes 0 ""))
       (state (retrodeck:dashboard-initial-state games))
       (*record-interaction* t)
       (*interaction-trace* nil)
       (*play-status* 1)
       (*active-status* 0)
       (presenter (lambda ()
                    (push :present *interaction-trace*)
                    t)))
  (multiple-value-bind (pressed pressed-layout effect)
      (retrodeck:apply-dashboard-touch games state layout
                                       '(1084 282 t t nil) 42 presenter)
    (assert (eq pressed-layout layout))
    (assert (null effect))
    (assert (null *interaction-trace*))
    (multiple-value-bind (released released-layout release-effect)
        (retrodeck:apply-dashboard-touch games pressed pressed-layout
                                         '(1084 282 nil nil t) 42 presenter)
      (assert (= (getf released :game-position) 1))
      (assert (= (getf released-layout :shown-game-index) 1))
      (assert (equal release-effect '(:render t :cue :next)))
      (assert (equal (reverse *interaction-trace*)
                     '(:render :present :sound)))
      (assert (equal *play-arguments* '(659 35 0 0 42)))

      (setf *interaction-trace* nil
            *active-status* 1
            *play-status* 2)
      (multiple-value-bind (pressed-again ignored-layout ignored-effect)
          (retrodeck:apply-dashboard-touch games released released-layout
                                           '(1084 282 t t nil) 42 presenter)
        (declare (ignore ignored-layout))
        (assert (null ignored-effect))
        (multiple-value-bind (released-again final-layout final-effect)
            (retrodeck:apply-dashboard-touch games pressed-again released-layout
                                             '(1084 282 nil nil t) 42 presenter)
          (assert (= (getf released-again :game-position) 2))
          (assert (= (getf final-layout :shown-game-index) 2))
          (assert (equal final-effect '(:render t :cue :next)))
          (assert (equal (reverse *interaction-trace*)
                         '(:render :present :sound)))))))
  (setf *active-status* 0
        *play-status* 1
        retrodeck::*menu-sound-input-until-ms* 0))

(let* ((games '((:id "alpha" :title "ALPHA" :system :nes :color #x5f87ff)
                (:id "beta" :title "BETA" :system :nes :color #xafd75f)
                (:id "long-title" :title "A VERY LONG FIXTURE GAME TITLE"
                 :system :nes :color #xffd700)
                (:id "delta" :title "DELTA" :system :nes :color #xd75f5f)
                (:id "gb" :title "GB FIXTURE" :system :gb :color #x87af87)
                (:id "gbc" :title "GBC FIXTURE" :system :gbc :color #xecb6e7)
                (:id "zx" :title "ZX FIXTURE" :system :zx :color #x87afff)
                (:id "chip8" :title "CHIP-8 FIXTURE" :system :chip8
                 :color #xffffaf)
                (:id "deck-fixture" :title "DECK FIXTURE" :system :deck
                 :color #xff8700)))
       (state (retrodeck:dashboard-initial-state games))
       (layout (retrodeck:render-dashboard games :nes 0 ""))
       (trace nil))
  (labels ((trace-touch (pressed-x pressed-y released-x released-y)
             (multiple-value-bind (pressed ignored)
                 (retrodeck:dashboard-touch-transition
                  state layout (list pressed-x pressed-y t t nil))
               (declare (ignore ignored))
               (multiple-value-bind (released effect)
                   (retrodeck:dashboard-touch-transition
                    pressed layout (list released-x released-y nil nil t))
                 (setf state released)
                 (when (getf effect :render)
                   (setf layout
                         (retrodeck:render-dashboard
                          games (getf state :active-system)
                          (getf state :game-position) (getf state :status))))
                 (push (list (getf state :active-system)
                             (getf state :game-position)
                             (not (null (getf effect :render)))
                             (getf effect :cue))
                       trace)))))
    (trace-touch 1084 282 1084 282)
    (trace-touch 1084 282 196 282)
    (trace-touch 1084 282 1084 282)
    (trace-touch 346 102 346 102)
    (trace-touch 346 102 346 102))
  ;; Shared with the C++ reference trace and its per-frame RGB565 hashes.
  (assert (equal (nreverse trace)
                 '((:nes 1 t :next)
                   (:nes 1 nil nil)
                   (:nes 2 t :next)
                   (:gb 0 t :next)
                   (:gb 0 t nil)))))

(let* ((games '((:id "alpha" :title "ALPHA" :system :nes
                 :color #x5f87ff)
                (:id "beta" :title "BETA" :system :nes
                 :color #xafd75f)
                (:id "gb" :title "GB" :system :gb :color #x87af87)))
       (layout (retrodeck:render-dashboard games :nes 0 ""))
       (state (retrodeck:dashboard-loop-initial-state
               games :network '(:wifi "TEST") :now 25)))
  (assert (eq (getf state :view) :dashboard))
  (assert (equal (getf state :network) '(:wifi "TEST")))
  (assert (= (getf state :network-refreshed-at) 25))
  (assert (eq (getf (getf state :settings) :selected) :volume-down))
  (assert (not (getf (getf state :settings) :open)))

  (let* ((armed (copy-list state))
         (dashboard (copy-list (getf state :dashboard))))
    (setf (getf dashboard :pressed-target) :settings
          (getf armed :dashboard) dashboard)
    (multiple-value-bind (next effects)
        (retrodeck:dashboard-reduce
         armed (list :controls :gamepad-actions '(:right)
                     :keyboard-actions nil :layout layout :now 100
                      :controller-quarantined-p nil))
      (assert (= (getf (getf next :dashboard) :game-position) 1))
      (assert (null (getf (getf next :dashboard) :pressed-target)))
      (assert (eq (getf (getf armed :dashboard) :pressed-target) :settings))
      (assert (equal effects
                     '((:discard-touch) (:render) (:present) (:cue :next))))))

  (multiple-value-bind (next effects)
      (retrodeck:dashboard-reduce
       state (list :controls :gamepad-actions '(:confirm)
                   :keyboard-actions '(:right) :layout layout :now 150
                   :controller-quarantined-p t))
    (assert (= (getf (getf next :dashboard) :game-position) 1))
    (assert (equal (getf (getf next :controller-guard) :edge-times) '(150)))
    (assert (equal effects
                   '((:discard-touch) (:render) (:present) (:cue :next)))))

  (multiple-value-bind (next effects)
      (retrodeck:dashboard-reduce
       state (list :controls :gamepad-actions '(:confirm)
                   :keyboard-actions nil :layout layout :now 175
                   :controller-quarantined-p t))
    (assert (zerop (getf (getf next :dashboard) :game-position)))
    (assert (equal (getf (getf next :controller-guard) :edge-times) '(175)))
    (assert (null effects)))

  (assert
   (signals-error-p
    (lambda ()
      (retrodeck:dashboard-reduce
       state (list :controls :gamepad-actions nil :keyboard-actions nil
                   :layout layout :now 190)))))

  (let ((suspended (copy-list state)))
    (setf (getf suspended :controller-guard)
          '(:edge-times (0) :suspended t :last-edge-at 100))
    (multiple-value-bind (waiting effects)
        (retrodeck:dashboard-reduce suspended '(:tick :now 1099))
      (assert (getf (getf waiting :controller-guard) :suspended))
      (assert (null effects)))
    (multiple-value-bind (recovered effects)
        (retrodeck:dashboard-reduce suspended '(:tick :now 1100))
      (assert (equal (getf recovered :controller-guard)
                     '(:edge-times nil :suspended nil :last-edge-at nil)))
      (assert (equal effects '((:controller-resumed))))))

  (multiple-value-bind (next effects)
      (retrodeck:dashboard-reduce
       state (list :controls :gamepad-actions '(:system-previous)
                   :keyboard-actions nil :layout layout :now 200
                    :controller-quarantined-p nil))
    (assert (eq (getf (getf next :dashboard) :active-system) :gb))
    (assert (equal effects
                   '((:discard-touch) (:render) (:present)
                     (:cue :previous)))))

  (multiple-value-bind (settings effects)
      (retrodeck:dashboard-reduce
       state (list :controls :gamepad-actions '(:settings)
                   :keyboard-actions nil :layout layout :now 250
                    :controller-quarantined-p nil))
    (assert (eq (getf settings :view) :settings))
    (assert (getf (getf settings :settings) :open))
    (assert (eq (getf (getf settings :settings) :selected) :volume-down))
    (assert (equal effects
                   '((:discard-touch) (:render) (:present)
                     (:cue :confirm))))
    (multiple-value-bind (moved move-effects)
        (retrodeck:dashboard-reduce
         settings (list :controls :gamepad-actions nil
                        :keyboard-actions '(:right) :layout layout :now 275
                        :controller-quarantined-p nil))
      (assert (eq (getf (getf moved :settings) :selected) :volume-up))
      (assert (equal move-effects
                     '((:discard-touch) (:render) (:present) (:cue :next))))
      (multiple-value-bind (closed close-effects)
          (retrodeck:dashboard-reduce
           moved (list :controls :gamepad-actions nil
                       :keyboard-actions '(:back) :layout layout :now 300
                       :controller-quarantined-p nil))
        (assert (eq (getf closed :view) :dashboard))
        (assert (equal close-effects
                       '((:discard-touch) (:render) (:present)
                         (:cue :back)))))))

  (multiple-value-bind (pressed effects)
      (retrodeck:dashboard-reduce
       state (list :touch :report '(1084 282 t t nil) :layout layout
                   :now 350))
    (assert (null effects))
    (multiple-value-bind (released release-effects)
        (retrodeck:dashboard-reduce
         pressed (list :touch :report '(1084 282 nil nil t) :layout layout
                       :now 351))
      (assert (= (getf (getf released :dashboard) :game-position) 1))
      (assert (equal release-effects
                     '((:render) (:present) (:cue :next))))))

  (let* ((modal (copy-list state))
         (credits (copy-list (getf state :credits))))
    (setf (getf modal :view) :credits
          (getf credits :pressed-target) :close
          (getf modal :credits) credits)
    (multiple-value-bind (next effects)
        (retrodeck:dashboard-reduce
         modal (list :controls :gamepad-actions '(:right)
                     :keyboard-actions nil :layout layout :now 400
                      :controller-quarantined-p nil))
      (assert (eq (getf (getf next :credits) :pressed-target) :close))
      (assert (null effects)))))

(labels ((touch-pair (state layout x y now)
           (multiple-value-bind (pressed press-effects)
               (retrodeck:dashboard-reduce
                state (list :touch :report (list x y t t nil)
                            :layout layout :now now))
             (assert (null press-effects))
             (retrodeck:dashboard-reduce
              pressed (list :touch :report (list x y nil nil t)
                            :layout layout :now (1+ now))))))
  (let* ((games '((:id "alpha" :title "ALPHA" :system :nes
                   :color #x5f87ff :rom "/tmp/alpha.nes")))
         (dashboard-layout (retrodeck:render-dashboard games :nes 0 ""))
         (state (retrodeck:dashboard-loop-initial-state
                 games :volume 42 :brightness 60 :keymap "us")))
    (multiple-value-bind (settings open-effects)
        (touch-pair state dashboard-layout 1220 420 1000)
      (assert (eq (getf settings :view) :settings))
      (assert (eq (getf (getf settings :settings) :selected) :volume-down))
      (assert (equal open-effects
                     '((:render) (:present) (:cue :confirm))))
      (let ((settings-layout
              (retrodeck:render-dashboard-settings 42 60 "us" :volume-down
                                                   "" nil)))
        (multiple-value-bind (requested request-effects)
            (retrodeck:dashboard-reduce
             settings
             (list :controls :gamepad-actions '(:confirm)
                   :keyboard-actions nil :layout settings-layout :now 1010
                   :controller-quarantined-p nil))
          (let ((plan (getf requested :pending-settings-plan)))
            (assert (eq (getf plan :action) :volume))
            (assert (= (getf plan :value) 37))
            (assert (equal request-effects
                           (list '(:discard-touch)
                                 (list :settings-action plan))))
            (assert
             (signals-error-p
              (lambda ()
                (retrodeck:dashboard-reduce
                 requested
                 (list :touch :report '(0 0 t t nil)
                       :layout settings-layout :now 1011)))))
            (multiple-value-bind (completed complete-effects)
                (retrodeck:dashboard-reduce
                 requested '(:settings-result :succeeded-p t))
              (assert (= (getf (getf completed :settings) :volume) 37))
              (assert (string= (getf (getf completed :settings) :status)
                               "GAME VOLUME 37%"))
              (assert (equal complete-effects
                             '((:render) (:present)
                               (:cue :volume :report-result t))))
              (multiple-value-bind (tone-failed tone-effects)
                  (retrodeck:dashboard-reduce
                   completed '(:volume-tone-result :succeeded-p nil))
                (assert
                 (string= (getf (getf tone-failed :settings) :status)
                          "VOLUME SAVED; CONFIRMATION TONE FAILED"))
                (assert (equal tone-effects '((:render) (:present))))))))

        (multiple-value-bind (wifi open-wifi-effects)
            (touch-pair settings settings-layout 1000 50 1020)
          (assert (eq (getf wifi :view) :wifi))
          (assert (getf (getf wifi :settings) :open))
          (assert (getf (getf wifi :wifi) :open))
          (assert (equal open-wifi-effects
                         '((:render) (:present) (:cue :confirm))))
          (let ((wifi-layout
                  (retrodeck:render-dashboard-wifi (getf wifi :wifi) nil)))
            (multiple-value-bind (focused focus-effects)
                (touch-pair wifi wifi-layout 340 20 1030)
              (assert (eq (getf (getf focused :wifi) :field) :ssid))
              (assert (equal focus-effects
                             '((:render) (:cue :next) (:present)))))
            (multiple-value-bind (unchanged blank-effects)
                (touch-pair wifi wifi-layout -1 -1 1035)
              (assert (eq (getf unchanged :view) :wifi))
              (assert (equal blank-effects '((:present)))))
            (multiple-value-bind (closed close-effects)
                (touch-pair wifi wifi-layout 20 20 1040)
              (assert (eq (getf closed :view) :settings))
              (assert (string= (getf (getf closed :settings) :status)
                               "WIFI EDITOR CLOSED"))
              (assert (equal close-effects
                             '((:render) (:cue :back) (:present))))))))))

  (let* ((games '((:id "alpha" :title "ALPHA" :system :nes
                   :color #x5f87ff)))
         (dashboard-layout (retrodeck:render-dashboard games :nes 0 ""))
         (state (retrodeck:dashboard-loop-initial-state
                 games :wifi-state
                 (retrodeck:wifi-initial-state
                  :ssid "DEMO" :passphrase "password"))))
    (multiple-value-bind (settings ignored)
        (touch-pair state dashboard-layout 1220 420 1100)
      (declare (ignore ignored))
      (let ((settings-layout
              (retrodeck:render-dashboard-settings
               42 100 "us" :volume-down "" nil)))
        (multiple-value-bind (wifi ignored)
            (touch-pair settings settings-layout 1000 50 1110)
          (declare (ignore ignored))
          (let ((wifi-layout
                  (retrodeck:render-dashboard-wifi (getf wifi :wifi) nil)))
            (multiple-value-bind (saving save-effects)
                (touch-pair wifi wifi-layout 1000 20 1120)
              (let ((plan (getf saving :pending-wifi-plan)))
                (assert (eq (getf plan :action) :save))
                (assert (equal save-effects
                               (list (list :wifi-action plan))))
                (assert
                 (signals-error-p
                  (lambda ()
                    (retrodeck:dashboard-reduce
                     saving
                     (list :touch :report '(20 20 nil nil t)
                           :layout wifi-layout :now 1122)))))
                (multiple-value-bind (saved completion-effects)
                    (retrodeck:dashboard-reduce
                     saving '(:wifi-result :succeeded-p t))
                  (assert (string= (getf (getf saved :wifi) :passphrase) ""))
                  (assert (string= (getf (getf saved :wifi) :status)
                                   "WIFI SAVED - USED AFTER CURRENT WIFI DISCONNECTS"))
                  (assert (equal completion-effects
                                 '((:render) (:cue :confirm) (:present))))))))))))

  (let* ((games '((:id "alpha" :title "ALPHA" :system :nes
                   :color #x5f87ff)))
         (layout (retrodeck:render-dashboard games :nes 0 ""))
         (state (retrodeck:dashboard-loop-initial-state games)))
    (multiple-value-bind (credits open-effects)
        (touch-pair state layout 20 420 1200)
      (assert (eq (getf credits :view) :credits))
      (assert (= (getf credits :credits-started-at) 1201))
      (assert (equal open-effects
                     '((:render) (:present) (:cue :confirm))))
      (multiple-value-bind (closed close-effects)
          (touch-pair credits '(:close (1212 12 56 56)) 1220 20 1210)
        (assert (eq (getf closed :view) :dashboard))
        (assert (equal close-effects
                       '((:render) (:present) (:cue :back)))))))

  (let* ((games (list '(:id "alpha" :title "ALPHA" :system :nes
                        :color #x5f87ff :rom "/tmp/alpha.nes")
                      (retrodeck:dashboard-application "terminal")
                      (retrodeck:dashboard-application "reboot")))
         (deck-layout (retrodeck:render-dashboard games :deck 1 ""))
         (state (retrodeck:dashboard-loop-initial-state games))
         (dashboard (copy-list (getf state :dashboard))))
    (setf (getf dashboard :active-system) :deck
          (getf dashboard :game-position) 1
          (getf state :dashboard) dashboard)
    (multiple-value-bind (armed arm-effects)
        (retrodeck:dashboard-reduce
         state (list :controls :gamepad-actions '(:confirm)
                     :keyboard-actions nil :layout deck-layout :now 1000
                     :controller-quarantined-p nil))
      (assert (= (getf armed :reboot-deadline) 5000))
      (assert (null (getf armed :pending-launch)))
      (assert (string= (getf (getf armed :dashboard) :status)
                       retrodeck:*dashboard-reboot-confirmation-text*))
      (assert (equal arm-effects
                     '((:discard-touch) (:render) (:present)
                       (:cue :confirm))))
      (multiple-value-bind (still-armed effects)
          (retrodeck:dashboard-reduce armed '(:tick :now 4999))
        (assert (= (getf still-armed :reboot-deadline) 5000))
        (assert (null effects)))
      (multiple-value-bind (expired effects)
          (retrodeck:dashboard-reduce armed '(:tick :now 5000))
        (assert (zerop (getf expired :reboot-deadline)))
        (assert (string= (getf (getf expired :dashboard) :status) ""))
        (assert (equal effects '((:render) (:present)))))
      (multiple-value-bind (confirmed confirm-effects)
          (retrodeck:dashboard-reduce
           armed (list :controls :gamepad-actions '(:confirm)
                       :keyboard-actions nil :layout deck-layout :now 4999
                       :controller-quarantined-p nil))
        (assert (equal (getf confirmed :pending-launch)
                       '(:kind :reboot :game-index 2 :touch-batch nil)))
        (assert (zerop (getf confirmed :reboot-deadline)))
        (assert (equal confirm-effects
                       '((:discard-touch) (:cue :confirm)))))
      (multiple-value-bind (pressed ignored)
          (retrodeck:dashboard-reduce
           armed (list :touch :report '(-1 -1 t t nil)
                       :layout deck-layout :now 4500))
        (declare (ignore ignored))
        (multiple-value-bind (cancelled effects)
            (retrodeck:dashboard-reduce
             pressed (list :touch :report '(-1 -1 nil nil t)
                           :layout deck-layout :now 4501))
          (assert (zerop (getf cancelled :reboot-deadline)))
          (assert (string= (getf (getf cancelled :dashboard) :status) ""))
          (assert (null effects)))))

    (let ((nes-layout (retrodeck:render-dashboard games :nes 0 "")))
      (multiple-value-bind (requested effects)
          (retrodeck:dashboard-reduce
           (retrodeck:dashboard-loop-initial-state games)
           (list :controls :gamepad-actions '(:confirm)
                 :keyboard-actions nil :layout nes-layout :now 1300
                 :controller-quarantined-p nil))
        (assert (equal (getf requested :pending-launch)
                       '(:kind :game :game-index 0 :touch-batch nil)))
        (assert (equal effects '((:discard-touch) (:cue :confirm))))))))

(let* ((games '((:id "alpha" :title "ALPHA" :system :nes
                 :color #x5f87ff :rom "/tmp/alpha.nes")))
       (layout (retrodeck:render-dashboard games :nes 0 ""))
       (state (retrodeck:dashboard-loop-initial-state games :now 100)))
  (assert (= (retrodeck:dashboard-loop-poll-timeout state) 250))
  (assert
   (signals-error-p
    (lambda ()
      (retrodeck:dashboard-reduce state '(:prepare-launch)))))
  (multiple-value-bind (touch-pressed ignored)
      (retrodeck:dashboard-reduce
       state (list :touch :report '(640 286 t t nil)
                   :layout layout :now 90))
    (declare (ignore ignored))
    (multiple-value-bind (touch-requested touch-effects)
        (retrodeck:dashboard-reduce
         touch-pressed (list :touch :report '(640 286 nil nil t)
                             :layout layout :now 91))
      (assert (getf (getf touch-requested :pending-launch) :touch-batch))
      (assert (equal touch-effects '((:cue :confirm))))
      (multiple-value-bind (continued effects)
          (retrodeck:dashboard-reduce
           touch-requested
           (list :touch :report '(-1 -1 nil nil t)
                 :layout layout :now 92))
        (assert (getf continued :pending-launch))
        (assert (null effects)))))
  (multiple-value-bind (settings ignored)
      (retrodeck:dashboard-reduce
       state (list :controls :gamepad-actions '(:settings)
                   :keyboard-actions nil :layout layout :now 101
                   :controller-quarantined-p nil))
    (declare (ignore ignored))
    (multiple-value-bind (early effects)
        (retrodeck:dashboard-reduce settings '(:tick :now 2099))
      (assert (null effects))
      (multiple-value-bind (refresh refresh-effects)
          (retrodeck:dashboard-reduce early '(:tick :now 2100))
        (assert (= (getf refresh :network-refreshed-at) 2100))
        (assert (getf refresh :pending-network))
        (assert (equal refresh-effects '((:network-action))))
        (assert
         (signals-error-p
          (lambda ()
            (retrodeck:dashboard-reduce refresh '(:tick :now 2101)))))
        (multiple-value-bind (unchanged unchanged-effects)
            (retrodeck:dashboard-reduce
             refresh '(:network-result :network nil))
          (assert (null unchanged-effects))
          (multiple-value-bind (again again-effects)
              (retrodeck:dashboard-reduce unchanged '(:tick :now 4100))
            (assert (equal again-effects '((:network-action))))
            (multiple-value-bind (changed changed-effects)
                (retrodeck:dashboard-reduce
                 again '(:network-result :network (:wifi "NEW")))
              (assert (equal (getf changed :network) '(:wifi "NEW")))
              (assert (equal changed-effects '((:render) (:present))))))))))

  (multiple-value-bind (lost effects)
      (retrodeck:dashboard-reduce state '(:touch-lost))
    (assert (string= (getf (getf lost :dashboard) :status)
                     "WAITING FOR TOUCHSCREEN"))
    (assert (equal effects '((:render) (:present))))
    (multiple-value-bind (restored restored-effects)
        (retrodeck:dashboard-reduce lost '(:touch-reconnected))
      (assert (string= (getf (getf restored :dashboard) :status)
                       "TOUCHSCREEN RECONNECTED"))
      (assert (equal restored-effects '((:render) (:present))))))

  (let ((credits (copy-list state)))
    (setf (getf credits :view) :credits)
    (assert (= (retrodeck:dashboard-loop-poll-timeout credits) 40))
    (multiple-value-bind (animated effects)
        (retrodeck:dashboard-reduce credits '(:tick :now 200))
      (declare (ignore animated))
      (assert (equal effects '((:render) (:present))))))
  (let ((credits (retrodeck:dashboard-loop-initial-state
                  games :reduced-motion t)))
    (setf (getf credits :view) :credits)
    (assert (= (retrodeck:dashboard-loop-poll-timeout credits) 250))
    (multiple-value-bind (static effects)
        (retrodeck:dashboard-reduce credits '(:tick :now 200))
      (declare (ignore static))
      (assert (null effects))))

  (multiple-value-bind (requested request-effects)
      (retrodeck:dashboard-reduce
       state (list :controls :gamepad-actions '(:confirm)
                   :keyboard-actions nil :layout layout :now 500
                   :controller-quarantined-p nil))
    (assert (equal request-effects '((:discard-touch) (:cue :confirm))))
    (assert
     (signals-error-p
      (lambda ()
        (retrodeck:dashboard-reduce requested '(:tick :now 501)))))
    (multiple-value-bind (launching launch-effects)
        (retrodeck:dashboard-reduce
         requested '(:prepare-launch :wayland t
                     :volume-state "/tmp/volume.state"))
      (let* ((launch (getf launching :active-launch))
             (plan (getf launch :plan)))
        (assert (eq (getf launch :kind) :game))
        (assert (string= (getf (getf launching :dashboard) :status)
                         "STARTING ALPHA"))
        (assert (equal launch-effects
                       (list '(:render) '(:present) '(:finish-sound)
                             '(:close-controls) (list :launch plan))))
        (assert (equal (cdr (assoc "RETRO_DECK_PRESENTATION"
                                   (getf plan :environment) :test #'string=))
                       "layer-shell"))
        (assert
         (signals-error-p
          (lambda ()
            (retrodeck:dashboard-reduce
             launching
             (list :controls :gamepad-actions nil :keyboard-actions '(:right)
                   :layout layout :now 501
                   :controller-quarantined-p nil)))))
        (multiple-value-bind (returned-child recovery-effects)
            (retrodeck:dashboard-reduce
             launching
             '(:child-returned
               :result (:started t :exited-for-touch t
                        :exit-code nil :signal nil :error nil)))
          (assert (equal recovery-effects '((:scan-controls :force t))))
          (multiple-value-bind (scanned scan-effects)
              (retrodeck:dashboard-reduce
               returned-child '(:controls-rescanned :now 600))
            (assert (equal scan-effects '((:open-presentation))))
            (multiple-value-bind (opened open-effects)
                (retrodeck:dashboard-reduce scanned '(:presentation-opened))
              (assert (equal open-effects '((:reload-volume))))
              (multiple-value-bind (returned return-effects)
                  (retrodeck:dashboard-reduce
                   opened '(:child-complete :volume 55))
                (assert (null (getf returned :active-launch)))
                (assert (= (getf (getf returned :settings) :volume) 55))
                (assert (= (getf (getf returned :settings)
                                 :last-audible-volume) 55))
                (assert (string= (getf (getf returned :dashboard) :status)
                                 "RETURNED FROM ALPHA"))
                (assert (equal return-effects
                               '((:render) (:present))))))))
        (let ((recovering (copy-list launching)))
          (setf (getf recovering :pending-child-result)
                '(:started t :exited-for-touch nil
                  :exit-code 7 :signal nil :error nil)
                (getf recovering :child-return-stage) :volume)
          (multiple-value-bind (failed ignored)
              (retrodeck:dashboard-reduce recovering '(:child-complete))
            (declare (ignore ignored))
            (assert (string= (getf (getf failed :dashboard) :status)
                             "ALPHA EXITED (STATUS 7)"))))
        (let ((recovering (copy-list launching)))
          (setf (getf recovering :pending-child-result)
                '(:started t :exited-for-touch nil
                  :exit-code nil :signal 15 :error nil)
                (getf recovering :child-return-stage) :volume)
          (multiple-value-bind (stopped ignored)
              (retrodeck:dashboard-reduce recovering '(:child-complete))
            (declare (ignore ignored))
            (assert (string= (getf (getf stopped :dashboard) :status)
                             "ALPHA STOPPED (SIGNAL 15)"))))))))

(format t "Lisp policy tests passed.~%")

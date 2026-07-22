(defpackage #:retrodeck.test
  (:use #:cl))

(in-package #:retrodeck.test)

(defparameter *play-status* 1)
(defparameter *play-arguments* nil)
(defparameter *active-status* 0)
(defparameter *stop-count* 0)
(defparameter *finish-count* 0)
(defparameter *canvas-clear-status* 1)
(defparameter *canvas-clear-color* nil)
(defparameter *canvas-glyph-status* 1)
(defparameter *canvas-glyph-arguments* nil)
(defparameter *canvas-glyph-calls* nil)
(defparameter *canvas-fill-status* 1)
(defparameter *canvas-fill-arguments* nil)
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

(defpackage #:retrodeck.native
  (:use)
  (:export #:abi-version
           #:audio-active-p
           #:canvas-clear
           #:canvas-draw-glyph
           #:canvas-draw-raster
           #:canvas-fill-rect
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
           #:stop-audio
           #:wayland-close
           #:wayland-dispatch
           #:wayland-next-touch
           #:wayland-open-widget
           #:wayland-present-canvas
           #:wayland-present-solid
           #:wayland-shutdown-p
           #:wayland-size))

(setf (symbol-function (find-symbol "ABI-VERSION" "RETRODECK.NATIVE"))
      (lambda () 7)
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
      (symbol-function (find-symbol "CANVAS-CLEAR" "RETRODECK.NATIVE"))
      (lambda (color)
        (setf *canvas-clear-color* color)
        *canvas-clear-status*)
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

(format t "Lisp policy tests passed.~%")

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

(format t "Lisp policy tests passed.~%")

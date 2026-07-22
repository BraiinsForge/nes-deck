(in-package #:retrodeck)

(defparameter *dashboard-systems*
  '((:nes "NES")
    (:gb "GAME BOY")
    (:gbc "GBC")
    (:zx "ZX SPECTRUM")
    (:chip8 "CHIP-8")
    (:deck "DECK")))

(defparameter *dashboard-palette*
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
    (:muted . #x949494)))

(defparameter *dashboard-executables*
  '((:nes . "/mnt/data/nes-deck/nes-deck")
    (:gb . "/mnt/data/nes-deck/gb-deck")
    (:zx . "/mnt/data/nes-deck/zx-deck")
    (:chip8 . "/mnt/data/nes-deck/chip8-deck")
    (:deck . "/mnt/data/nes-deck/ten-seconds-deck")
    (:chiptunes . "/mnt/data/nes-deck/chiptune-deck")
    (:terminal . "/mnt/data/nes-deck/terminal/retro-terminal")
    (:reboot . "/sbin/reboot")))

(defparameter *dashboard-built-in-applications*
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
     :color #xd75f5f)))

(defparameter *dashboard-timings*
  '((:child-touch-exit-ms . 2000)
    (:child-term-grace-ms . 4000)
    (:reboot-confirm-ms . 4000)
    (:controller-burst-window-ms . 1000)
    (:controller-quiet-reset-ms . 1000)
    (:main-poll-ms . 250)
    (:animated-poll-ms . 40)
    (:network-refresh-ms . 2000)
    (:console-mirror-ms . 100)))

(defparameter *dashboard-volume-default* 42)
(defparameter *dashboard-volume-step* 5)
(defparameter *dashboard-brightness-minimum* 10)
(defparameter *dashboard-brightness-step* 10)
(defparameter *dashboard-controller-burst-limit* 12)
(defparameter *dashboard-reboot-confirmation-text*
  "PRESS A OR TAP AGAIN TO REBOOT")
(defparameter *dashboard-terminal-login-shell* "/BIN/ASH")
(defparameter *dashboard-reduced-motion-environment*
  "RETRO_DECK_REDUCED_MOTION")

(defun dashboard-system-label (system)
  (let* ((name (etypecase system
                 (string system)
                 (symbol (string-downcase (symbol-name system)))))
         (definition
           (find name *dashboard-systems*
                 :key (lambda (entry)
                        (string-downcase (symbol-name (first entry))))
                 :test #'string=)))
    (if definition
        (second definition)
        (map 'string
             (lambda (character)
               (if (< (char-code character) 128) character #\?))
             name))))

(defun dashboard-color (role)
  (let ((entry (assoc role *dashboard-palette* :test #'eq)))
    (if entry
        (cdr entry)
        (error "Unknown dashboard color role ~S" role))))

(defun dashboard-timing (name)
  (let ((entry (assoc name *dashboard-timings* :test #'eq)))
    (if entry
        (cdr entry)
        (error "Unknown dashboard timing ~S" name))))

(defun dashboard-executable (route)
  (let ((entry (assoc route *dashboard-executables* :test #'eq)))
    (if entry
        (cdr entry)
        (error "Unknown dashboard executable route ~S" route))))

(defun dashboard-application (id)
  (let ((application
          (find id *dashboard-built-in-applications*
                :key (lambda (entry) (getf entry :id))
                :test #'string=)))
    (and application (copy-tree application))))

(defun dashboard-application-id-p (application id)
  (and (eq (getf application :system) :deck)
       (string= (getf application :id) id)))

(defun dashboard-application-route (application)
  (case (getf application :system)
    (:nes :nes)
    ((:gb :gbc) :gb)
    (:zx :zx)
    (:deck (if (dashboard-application-id-p application "chiptunes")
               :chiptunes
               :deck))
    (otherwise :chip8)))

(defun dashboard-launch-plan (application volume-percent
                              &key (keymap "us") wayland volume-state)
  (check-type volume-percent (integer 0 100))
  (check-type keymap string)
  (when volume-state
    (check-type volume-state string))
  (let ((terminal-mode (getf application :terminal-mode)))
    (cond
      (terminal-mode
       (list :executable (dashboard-executable :terminal)
             :arguments (list terminal-mode)
             :environment (list (cons "RETRO_DECK_KEYMAP" keymap))
             :label (if (string= terminal-mode "shell")
                        "terminal"
                        (format nil "~A REPL" terminal-mode))
             :touch-supervision t
             :mirror-console t))
      ((dashboard-application-id-p application "reboot")
       (list :executable (getf application :rom)
             :arguments nil
             :environment nil
             :label "reboot"
             :touch-supervision t
             :mirror-console nil))
      (t
       (let* ((system (getf application :system))
              (route (dashboard-application-route application))
              (arguments
                (if (or (not (eq system :deck))
                        (eq route :chiptunes))
                    (list (getf application :rom))
                    nil))
              (environment
                (list (cons "RETRO_DECK_VOLUME_PERCENT"
                            (format nil "~D" volume-percent)))))
         (unless (eq system :deck)
           (setf environment
                 (append environment
                         (list (cons "RETRO_DECK_EXIT_HINT" "1")))))
         (when wayland
           (setf environment
                 (append environment
                         (list (cons "RETRO_DECK_PRESENTATION"
                                     "layer-shell")))))
         (when (and volume-state (plusp (length volume-state)))
           (setf environment
                 (append environment
                         (list (cons "RETRO_DECK_VOLUME_STATE"
                                     volume-state)))))
         (list :executable (dashboard-executable route)
               :arguments arguments
               :environment environment
               :label (getf application :id)
               :touch-supervision (not (eq system :deck))
               :mirror-console nil))))))

(defun reboot-confirmation-active-p (deadline now)
  (and (plusp deadline) (< now deadline)))

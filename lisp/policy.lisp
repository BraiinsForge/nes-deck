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

(defparameter *dashboard-cover-directory* "/mnt/data/nes-deck/covers/")
(defparameter *dashboard-settings-icon-path*
  "/mnt/data/nes-deck/menu/settings-icon.png")

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
    (:controller-scan-ms . 1000)
    (:touch-reconnect-ms . 1000)
    (:main-poll-ms . 250)
    (:animated-poll-ms . 40)
    (:network-refresh-ms . 2000)
    (:console-mirror-ms . 100)))

(defparameter *dashboard-volume-default* 42)
(defparameter *dashboard-volume-step* 5)
(defparameter *dashboard-brightness-minimum* 10)
(defparameter *dashboard-brightness-step* 10)
(defparameter *dashboard-controller-burst-limit* 12)

(defparameter *dashboard-keyboard-controls*
  '((1 :back)
    (15 :system-next :system-previous)
    (28 :confirm)
    (96 :confirm)
    (103 :up)
    (105 :left)
    (106 :right)
    (108 :down)))

(defparameter *dashboard-gamepad-controls*
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
    (#x800 . :down)))

(defun dashboard-control-actions (report)
  (check-type report list)
  (case (getf report :kind)
    (:keyboard
     (let* ((definition (assoc (getf report :code)
                               *dashboard-keyboard-controls* :test #'=))
            (action (and definition
                         (if (and (getf report :shift) (third definition))
                             (third definition)
                             (second definition)))))
       (and action (list action))))
    (:gamepad
     (let ((edges (getf report :edges)))
       (check-type edges (integer 1 4095))
       (remove-duplicates
        (loop for (mask . action) in *dashboard-gamepad-controls*
              when (logtest mask edges)
                collect action)
        :test #'eq)))
    (otherwise
     (error "Unknown dashboard control report ~S" report))))

(defun collect-dashboard-control-actions ()
  (let ((gamepad nil)
        (keyboard nil)
        (report-count 0))
    (loop for report = (next-evdev-control)
          while report
          do (incf report-count)
             (dolist (action (dashboard-control-actions report))
               (if (eq (getf report :kind) :gamepad)
                   (pushnew action gamepad :test #'eq)
                   (pushnew action keyboard :test #'eq))))
    (values gamepad keyboard report-count)))

(defun dashboard-controller-guard-initial-state ()
  (list :edge-times nil :suspended nil :last-edge-at nil))

(defun dashboard-controller-guard-accept-edge (state now)
  (check-type state list)
  (check-type now (integer 0 *))
  (let ((next (copy-list state)))
    (setf (getf next :last-edge-at) now)
    (when (getf next :suspended)
      (return-from dashboard-controller-guard-accept-edge
        (values next nil nil)))
    (let* ((window (dashboard-timing :controller-burst-window-ms))
           (edge-times
             (remove-if (lambda (time) (>= (- now time) window))
                        (getf next :edge-times)))
           (updated (append edge-times (list now))))
      (setf (getf next :edge-times) updated)
      (if (<= (length updated) *dashboard-controller-burst-limit*)
          (values next t nil)
          (progn
            (setf (getf next :suspended) t)
            (values next nil t))))))

(defun dashboard-controller-guard-recover-if-quiet (state now)
  (check-type state list)
  (check-type now (integer 0 *))
  (let ((last-edge-at (getf state :last-edge-at)))
    (if (and (getf state :suspended)
             (integerp last-edge-at)
             (>= (- now last-edge-at)
                 (dashboard-timing :controller-quiet-reset-ms)))
        (values (dashboard-controller-guard-initial-state) t)
        (values (copy-list state) nil))))

(defun dashboard-controller-scan-due-p (last-scan-ms now
                                         &key force rescan)
  (check-type last-scan-ms (or null (integer 0 *)))
  (check-type now (integer 0 *))
  (or force rescan (null last-scan-ms) (zerop last-scan-ms)
      (>= (- now last-scan-ms) (dashboard-timing :controller-scan-ms))))

(defun dashboard-controller-input-actions
    (gamepad-actions keyboard-actions guard now
     &key (controller-quarantined-p
            (menu-sound-blocks-input-p :controller now)))
  (check-type gamepad-actions list)
  (check-type keyboard-actions list)
  (let ((next-guard guard)
        (accepted-gamepad gamepad-actions)
        (newly-suspended nil))
    (when gamepad-actions
      (multiple-value-bind (updated accepted suspended)
          (dashboard-controller-guard-accept-edge guard now)
        (setf next-guard updated
              newly-suspended suspended)
        (unless accepted
          (setf accepted-gamepad nil))))
    (when controller-quarantined-p
      (setf accepted-gamepad nil))
    (values (remove-duplicates
             (append accepted-gamepad keyboard-actions) :test #'eq)
            next-guard newly-suspended)))

(defun dashboard-controller-command (actions modal-view settings-view)
  (check-type actions list)
  (cond
    ((and (or modal-view settings-view) (member :back actions)) :back)
    (modal-view nil)
    ((member :settings actions) :settings)
    ((and (not settings-view) (member :system-previous actions))
     :system-previous)
    ((and (not settings-view) (member :system-next actions)) :system-next)
    ((or (member :left actions) (member :up actions)) :previous)
    ((or (member :right actions) (member :down actions)) :next)
    ((member :confirm actions) :confirm)
    (t nil)))

(defparameter *dashboard-reboot-confirmation-text*
  "PRESS A OR TAP AGAIN TO REBOOT")
(defparameter *dashboard-loop-labels*
  '(:touch-waiting "WAITING FOR TOUCHSCREEN"
    :touch-reconnected "TOUCHSCREEN RECONNECTED"
    :rebooting "REBOOTING"
    :reboot-error "REBOOT ERROR - CHECK LOG"
    :reboot-not-started "REBOOT DID NOT START"
    :reboot-cancelled "REBOOT CANCELLED"
    :reboot-exited "REBOOT COMMAND EXITED"
    :game-error "GAME ERROR - CHECK LOG"
    :game-not-started "GAME DID NOT START"))
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

(defun dashboard-loop-label (name)
  (let ((value (getf *dashboard-loop-labels* name)))
    (if value
        value
        (error "Unknown dashboard loop label ~S" name))))

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

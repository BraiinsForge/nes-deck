(in-package #:retrodeck)

(defparameter *dashboard-menu-geometry*
  '(:credits (12 412 56 56)
    :settings (1212 412 56 56)
    :previous (156 232 80 100)
    :next (1044 232 80 100)
    :tabs (56 76 1168 52 8)
    :cards (216 264 36 154)
    :indicators (16 8 8 438)
    :status (100 452 1080 24 220)
    :pixel-stroke 4
    :title-scale 2))

(defparameter *dashboard-arrow-blocks*
  '((28 -2 4 4) (24 -6 4 4) (20 -10 4 4) (16 -14 4 4)
    (12 -18 4 4) (8 -22 4 10) (-28 -12 36 4) (-28 -8 4 16)
    (-28 8 36 4) (8 12 4 10) (12 14 4 4) (16 10 4 4)
    (20 6 4 4) (24 2 4 4)))

(defparameter *dashboard-settings-icon-fallback*
  '("..........000.........." ".........03320........."
    "...000...03220...000..." "..0333000222220003220.."
    "..0322222222222232220.." "..0322222222222222210.."
    "...03222100000222210..." "...0222100.1.0022220..."
    "...022100..1..002220..." ".0022200...1...0022200."
    "0332220...010...0222220" "03222201111011110222220"
    "0222220...010...0222210" ".0022200...1...0022200."
    "...022200..1..003220..." "...0222200.1.0032220..."
    "...03222200000322220..." "..0322222222222222220.."
    "..0222222222222222220.." "..0221000222220003110.."
    "...000...02220...000..." ".........02210........."
    "..........000.........."))

(defparameter *dashboard-raster-cache* (make-hash-table :test #'equal))

(defun clear-dashboard-raster-cache ()
  (let ((cleared (= (retrodeck.native:raster-clear) 1)))
    (clrhash *dashboard-raster-cache*)
    cleared))

(defun dashboard-cached-raster (key loader)
  (multiple-value-bind (handle present-p)
      (gethash key *dashboard-raster-cache*)
    (if present-p
        handle
        (setf (gethash key *dashboard-raster-cache*) (funcall loader)))))

(defun dashboard-settings-raster ()
  (let ((path (namestring (pathname *dashboard-settings-icon-path*))))
    (dashboard-cached-raster
     (list :png path 23 23)
     (lambda () (load-png-raster path 23 23)))))

(defun dashboard-cover-path (game)
  (or (getf game :cover)
      (merge-pathnames (concatenate 'string (getf game :id) ".png")
                       (pathname *dashboard-cover-directory*))))

(defun dashboard-cover-raster (game)
  (let* ((path (namestring (pathname (dashboard-cover-path game))))
         (color (getf game :color)))
    (dashboard-cached-raster
     (list :cover path color)
     (lambda () (load-cover-raster path color)))))

(defun prepare-dashboard-rasters (games)
  (check-type games list)
  (dashboard-settings-raster)
  (dolist (game games t)
    (dashboard-cover-raster game)))

(defun dashboard-menu-geometry (name)
  (let ((value (getf *dashboard-menu-geometry* name)))
    (if value
        (if (listp value) (copy-list value) value)
        (error "Unknown dashboard geometry ~S" name))))

(defun dashboard-populated-systems (games)
  (loop for definition in *dashboard-systems*
        for system = (first definition)
        when (find system games :key (lambda (game) (getf game :system))
                                  :test #'eq)
          collect system))

(defun draw-dashboard-settings-fallback (x y width height)
  (let* ((source-size 23)
         (target-size (max 1 (min 50 width height)))
         (left (+ x (floor (- width target-size) 2)))
         (top (+ y (floor (- height target-size) 2)))
         (colors #(#x000000 #x2e2e2e #x727272 #xa0a0a0)))
    (dotimes (target-y target-size)
      (let ((row (nth (floor (* target-y source-size) target-size)
                      *dashboard-settings-icon-fallback*))
            (target-x 0))
        (loop while (< target-x target-size) do
          (let* ((shade (char row (floor (* target-x source-size)
                                         target-size)))
                 (end (1+ target-x)))
            (loop while (and (< end target-size)
                             (char= shade
                                    (char row (floor (* end source-size)
                                                     target-size))))
                  do (incf end))
            (unless (char= shade #\.)
              (fill-canvas-rect (+ left target-x) (+ top target-y)
                                (- end target-x) 1
                                (aref colors (- (char-code shade)
                                                (char-code #\0)))))
            (setf target-x end)))))
    t))

(defun draw-dashboard-settings-icon (x y width height)
  (let* ((target-size (max 1 (min 50 width height)))
         (left (+ x (floor (- width target-size) 2)))
         (top (+ y (floor (- height target-size) 2)))
         (raster (dashboard-settings-raster)))
    (or (and raster
             (draw-canvas-raster raster left top target-size target-size))
        (draw-dashboard-settings-fallback x y width height))))

(defun draw-dashboard-outline-arrow (x y width height direction color)
  (let ((center-x (+ x (floor width 2)))
        (center-y (+ y (floor height 2)))
        (mirror (if (eq direction :left) -1 1)))
    (dolist (block *dashboard-arrow-blocks* t)
      (destructuring-bind (block-x block-y block-width block-height) block
        (fill-canvas-rect
         (if (minusp mirror)
             (- center-x block-x block-width)
             (+ center-x block-x))
         (+ center-y block-y) block-width block-height color)))))

(defun draw-dashboard-power-icon (x y width height color)
  (let ((center-x (+ x (floor width 2)))
        (center-y (+ y (floor height 2))))
    (dolist (rect `((,(- center-x 5) ,(- center-y 58) 10 54)
                    (,(- center-x 48) ,(- center-y 34) 22 8)
                    (,(+ center-x 26) ,(- center-y 34) 22 8)
                    (,(- center-x 58) ,(- center-y 26) 8 54)
                    (,(+ center-x 50) ,(- center-y 26) 8 54)
                    (,(- center-x 48) ,(+ center-y 28) 16 8)
                    (,(+ center-x 32) ,(+ center-y 28) 16 8)
                    (,(- center-x 32) ,(+ center-y 36) 64 8))
             t)
      (apply #'fill-canvas-rect (append rect (list color))))))

(defun dashboard-built-in-game-p (game id)
  (and (eq (getf game :system) :deck)
       (string= (getf game :id) id)))

(defun draw-dashboard-compact-logo (x y width height game)
  (unless (eq (getf game :system) :deck)
    (return-from draw-dashboard-compact-logo nil))
  (let ((id (getf game :id))
        (color (getf game :color))
        (center-x (+ x (floor width 2)))
        (center-y (+ y (floor height 2))))
    (cond
      ((string= id "ten-seconds")
       (draw-centered-text x y width height "10.00" 5 color))
      ((dashboard-built-in-game-p game "lua-repl")
       (draw-pixel-panel (+ x 24) (+ y 46) (- width 48) (- height 92)
                         (dashboard-color :background) color 4)
       (draw-centered-text x y width height "LUA>" 4 color))
      ((dashboard-built-in-game-p game "lisp-repl")
       (draw-centered-text x y width height "(LISP)" 4 color))
      ((dashboard-built-in-game-p game "python-repl")
       (draw-centered-text x y width height ">>>" 6 color)
       (fill-canvas-rect (- center-x 54) (+ center-y 34) 108 6 color))
      ((dashboard-built-in-game-p game "scheme-repl")
       (draw-centered-text x y width height "(SCHEME)" 3 color)
       (fill-canvas-rect (- center-x 34) (+ center-y 30) 68 5 color))
      ((dashboard-built-in-game-p game "chiptunes")
       (loop for bar-height in '(34 62 92 48 112 74 42 86 56)
             for index from 0
             do (fill-canvas-rect (+ center-x -86 (* index 20))
                                  (- center-y (floor bar-height 2))
                                  10 bar-height color)))
      ((dashboard-built-in-game-p game "terminal")
       (let ((screen-x (+ x 30))
             (screen-y (+ y 44))
             (screen-width (- width 60)))
         (stroke-canvas-rect screen-x screen-y screen-width 96 4 color)
         (draw-centered-text screen-x screen-y screen-width 96 ">_" 5
                             (dashboard-color :text))
         (fill-canvas-rect (- center-x 6) (+ screen-y 96) 12 18 color)
         (fill-canvas-rect (- center-x 44) (+ screen-y 114) 88 4 color)))
      ((dashboard-built-in-game-p game "reboot")
       (draw-dashboard-power-icon x y width height color))
      (t (return-from draw-dashboard-compact-logo nil)))
    t))

(defun draw-dashboard-cartridge (x y width height color)
  (let ((cartridge-x (+ x 34))
        (cartridge-y (+ y 28))
        (cartridge-width (- width 68))
        (cartridge-height (- height 56)))
    (draw-pixel-panel cartridge-x cartridge-y cartridge-width cartridge-height
                      (dashboard-color :background) color 4)
    (fill-canvas-rect (+ cartridge-x 24) (+ cartridge-y 26)
                      (- cartridge-width 48) 8 color)
    (fill-canvas-rect (+ cartridge-x 24) (+ cartridge-y 46)
                      (- cartridge-width 48) 4 color)
    (fill-canvas-rect (+ cartridge-x 20)
                      (+ cartridge-y cartridge-height -30)
                      (- cartridge-width 40) 10 color)
    t))

(defun draw-dashboard-game-card (x y width height game selected)
  (let* ((stroke (dashboard-menu-geometry :pixel-stroke))
         (art-x (+ x 8))
         (art-y (+ y 8))
         (art-size (- width 16))
         (label-x (+ x 8))
         (label-y (+ y width))
         (label-width (- width 16))
         (label-height (- height width 8))
         (color (getf game :color)))
    (draw-pixel-panel x y width height
                      (dashboard-color (if selected :active :background))
                      (dashboard-color :accent) stroke)
    (let ((cover (dashboard-cover-raster game)))
      (unless (and cover
                   (draw-canvas-raster cover art-x art-y art-size art-size))
        (unless (draw-dashboard-compact-logo art-x art-y art-size art-size game)
          (draw-dashboard-cartridge art-x art-y art-size art-size color))))
    (draw-centered-text label-x label-y label-width label-height
                        (fit-text-width (getf game :title)
                                        (- label-width 12)
                                        (dashboard-menu-geometry :title-scale))
                        (dashboard-menu-geometry :title-scale)
                        (dashboard-color :text))))

(defun draw-dashboard-tabs (games active-system)
  (destructuring-bind (left top total-width height gap)
      (dashboard-menu-geometry :tabs)
    (let* ((systems (dashboard-populated-systems games))
           (count (length systems))
           (width (if (zerop count)
                      0
                      (floor (- total-width (* gap (1- count))) count)))
           (buttons nil))
      (loop for system in systems
            for index from 0
            for x = (+ left (* index (+ width gap)))
            for bounds = (list x top width height)
            do (push bounds buttons)
               (draw-pixel-panel x top width height
                                 (dashboard-color
                                  (if (eq system active-system)
                                      :active :background))
                                 (dashboard-color :accent)
                                 (dashboard-menu-geometry :pixel-stroke))
               (let ((label (dashboard-system-label system)))
                 (draw-centered-text x top width height label
                                     (fit-text-scale label (- width 16) 2 1)
                                     (dashboard-color :text))))
      (values systems (nreverse buttons)))))

(defun dashboard-active-game-pairs (games active-system)
  (loop for game in games
        for index from 0
        when (eq (getf game :system) active-system)
          collect (cons index game)))

(defun draw-dashboard-carousel (games active-system game-position)
  (let* ((pairs (dashboard-active-game-pairs games active-system))
         (count (length pairs))
         (previous (dashboard-menu-geometry :previous))
         (next (dashboard-menu-geometry :next))
         (visible-indices nil)
         (game-buttons nil)
         (indicator-buttons nil)
         (shown-index (length games)))
    (when (plusp count)
      (let* ((selected-position (mod game-position count))
             (selected (nth selected-position pairs))
             (visible-count (min 3 count))
             (first-position
               (cond ((<= count visible-count) 0)
                     ((zerop selected-position) 0)
                     ((>= (1+ selected-position) count)
                      (- count visible-count))
                     (t (1- selected-position))))
             (visible (subseq pairs first-position
                              (+ first-position visible-count))))
        (setf shown-index (car selected))
        (destructuring-bind (card-width card-height gap top)
            (dashboard-menu-geometry :cards)
          (let* ((row-width (+ (* visible-count card-width)
                               (* (1- visible-count) gap)))
                 (x (floor (- 1280 row-width) 2)))
            (dolist (pair visible)
              (let ((bounds (list x top card-width card-height)))
                (push (car pair) visible-indices)
                (push bounds game-buttons)
                (draw-dashboard-game-card x top card-width card-height
                                          (cdr pair)
                                          (= (car pair) shown-index))
                (incf x (+ card-width gap))))))
        (if (> count 1)
            (progn
              (apply #'draw-dashboard-outline-arrow
                     (append previous
                             (list :left (dashboard-color :footer))))
              (apply #'draw-dashboard-outline-arrow
                     (append next
                             (list :right (dashboard-color :footer)))))
            (setf previous '(0 0 0 0)
                  next '(0 0 0 0)))
        (destructuring-bind (width height gap top)
            (dashboard-menu-geometry :indicators)
          (let* ((row-width (+ (* count width) (* (max 0 (1- count)) gap)))
                 (x (floor (- 1280 row-width) 2)))
            (dotimes (indicator count)
              (let ((bounds (list x top width height)))
                (push bounds indicator-buttons)
                (stroke-canvas-rect x top width height 2
                                    (dashboard-color
                                     (if (= indicator selected-position)
                                         :footer :control-border)))
                (incf x (+ width gap))))))))
    (list :game-indices (mapcar #'car pairs)
          :visible-game-indices (nreverse visible-indices)
          :game-buttons (nreverse game-buttons)
          :indicators (nreverse indicator-buttons)
          :shown-game-index shown-index
          :previous previous
          :next next)))

(defun render-dashboard (games active-system game-position status)
  (check-type games list)
  (check-type game-position (integer 0 *))
  (check-type status string)
  (prepare-dashboard-rasters games)
  (clear-canvas (dashboard-color :background))
  (let ((credits (dashboard-menu-geometry :credits))
        (settings (dashboard-menu-geometry :settings)))
    (apply #'draw-centered-text
           (append credits (list "(c)" 2 (dashboard-color :footer))))
    (apply #'draw-dashboard-settings-icon settings)
    (multiple-value-bind (systems system-buttons)
        (draw-dashboard-tabs games active-system)
      (let ((carousel
              (draw-dashboard-carousel games active-system game-position)))
        (unless (zerop (length status))
          (destructuring-bind (x y width height margin)
              (dashboard-menu-geometry :status)
            (draw-centered-text x y width height status
                                (fit-text-scale status (- 1280 margin) 2 1)
                                (dashboard-color :footer))))
        (list :credits credits
              :settings settings
              :previous (getf carousel :previous)
              :next (getf carousel :next)
              :systems systems
              :system-buttons system-buttons
              :game-indices (getf carousel :game-indices)
              :visible-game-indices (getf carousel :visible-game-indices)
              :game-buttons (getf carousel :game-buttons)
              :indicators (getf carousel :indicators)
              :shown-game-index (getf carousel :shown-game-index))))))

(defun dashboard-initial-system (games)
  (or (first (dashboard-populated-systems games))
      (getf (first games) :system)))

(defun dashboard-initial-state (games)
  (check-type games list)
  (list :active-system (dashboard-initial-system games)
        :game-position 0
        :pressed-target nil
        :status ""))

(defun dashboard-bounds-contains-p (bounds x y)
  (and bounds
       (destructuring-bind (left top width height) bounds
         (and (<= left x) (< x (+ left width))
              (<= top y) (< y (+ top height))))))

(defun dashboard-target-at (layout x y)
  (check-type layout list)
  (check-type x integer)
  (check-type y integer)
  (or (loop for (key target) in '((:credits :credits)
                                  (:settings :settings)
                                  (:previous :previous)
                                  (:next :next))
            when (dashboard-bounds-contains-p (getf layout key) x y)
              return target)
      (loop for system in (getf layout :systems)
            for bounds in (getf layout :system-buttons)
            when (dashboard-bounds-contains-p bounds x y)
              return (list :system system))
      (loop for game-index in (getf layout :visible-game-indices)
            for bounds in (getf layout :game-buttons)
            when (dashboard-bounds-contains-p bounds x y)
              return (list :game game-index))))

(defun dashboard-touch-transition (state layout report)
  (check-type state list)
  (check-type layout list)
  (destructuring-bind (x y down pressed released) report
    (check-type x integer)
    (check-type y integer)
    (check-type down boolean)
    (check-type pressed boolean)
    (check-type released boolean)
    (let* ((next (copy-list state))
           (game-position (getf next :game-position))
           (target (dashboard-target-at layout x y))
           (effect nil))
      (check-type game-position (integer 0 *))
      (when pressed
        (setf (getf next :pressed-target) target))
      (when released
        (let ((pressed-target (getf next :pressed-target)))
          (setf (getf next :pressed-target) nil)
          (when (equal pressed-target target)
            (cond
              ((and (consp target) (eq (first target) :system))
               (let* ((requested-system (second target))
                      (moved (not (equal requested-system
                                         (getf next :active-system)))))
                 (setf (getf next :active-system) requested-system
                       (getf next :game-position) 0
                       (getf next :status) ""
                       effect (if moved
                                  '(:render t :cue :next)
                                  '(:render t)))))
              ((member target '(:previous :next))
               (let ((count (length (getf layout :game-indices))))
                 (when (plusp count)
                   (setf (getf next :game-position)
                         (if (eq target :previous)
                             (if (zerop game-position)
                                 (1- count)
                                 (1- game-position))
                             (mod (1+ game-position) count))
                         (getf next :status) ""
                         effect (list :render t :cue target)))))))))
      (values next effect))))

(defun dashboard-adjacent-system (systems active-system direction)
  (check-type systems list)
  (check-type direction (member -1 1))
  (if (null systems)
      active-system
      (let* ((count (length systems))
             (position (or (position active-system systems :test #'eq) 0))
             (next-position
               (if (minusp direction)
                   (if (zerop position) (1- count) (1- position))
                   (mod (1+ position) count))))
        (nth next-position systems))))

(defun dashboard-loop-initial-state
    (games &key (volume *dashboard-volume-default*) last-audible-volume
                (brightness 100) (keymap "us") (network nil)
                (wifi-state (wifi-initial-state))
                (credits-state (credits-initial-state)) credits-crawl
                reduced-motion (now 0))
  (check-type games list)
  (check-type network list)
  (check-type now (integer 0 *))
  (let ((settings (settings-initial-state
                   :volume volume :last-audible-volume last-audible-volume
                   :brightness brightness :keymap keymap))
        (wifi (copy-list wifi-state)))
    (setf (getf settings :open) nil
          (getf wifi :open) nil)
    (list :games (copy-tree games)
          :view :dashboard
          :dashboard (dashboard-initial-state games)
          :settings settings
          :wifi wifi
          :credits (copy-list credits-state)
          :network (copy-tree network)
          :credits-crawl credits-crawl
          :credits-started-at 0
          :reduced-motion (not (null reduced-motion))
          :controller-guard (dashboard-controller-guard-initial-state)
          :last-control-scan-ms nil
          :network-refreshed-at now
          :reboot-deadline 0
          :pending-launch nil
          :pending-settings-plan nil
          :pending-volume-tone nil
          :pending-wifi-plan nil)))

(defun dashboard-loop-set-global-status (state status)
  (check-type status string)
  (let ((next (copy-list state))
        (dashboard (copy-list (getf state :dashboard)))
        (settings (copy-list (getf state :settings))))
    (setf (getf dashboard :status) status
          (getf settings :status) status
          (getf next :dashboard) dashboard
          (getf next :settings) settings)
    next))

(defun dashboard-loop-clear-pressed-targets (state)
  (let ((next (copy-list state)))
    (dolist (key '(:dashboard :settings :wifi :credits) next)
      (let ((slice (copy-list (getf state key))))
        (setf (getf slice :pressed-target) nil
              (getf next key) slice)))))

(defun dashboard-loop-set-view (state view)
  (unless (member view '(:dashboard :settings :wifi :credits))
    (error "Unknown dashboard view ~S" view))
  (let* ((next (dashboard-loop-clear-pressed-targets state))
         (settings (copy-list (getf next :settings)))
         (wifi (copy-list (getf next :wifi))))
    (setf (getf settings :open)
          (not (null (member view '(:settings :wifi))))
          (getf wifi :open) (eq view :wifi)
          (getf next :settings) settings
          (getf next :wifi) wifi
          (getf next :view) view)
    next))

(defun dashboard-loop-cancel-reboot (state)
  (let ((next (copy-list state)))
    (setf (getf next :reboot-deadline) 0)
    (if (string= (getf (getf state :dashboard) :status)
                 *dashboard-reboot-confirmation-text*)
        (dashboard-loop-set-global-status next "")
        next)))

(defun dashboard-loop-screen-effects (&optional cue)
  (append '((:render) (:present))
          (and cue (list (list :cue cue)))))

(defun dashboard-loop-transition-effects (effect)
  (when effect
    (append (and (getf effect :render) '((:render) (:present)))
            (and (getf effect :cue)
                 (list (list :cue (getf effect :cue)))))))

(defun dashboard-loop-request-game (state game-index now)
  (check-type game-index (integer 0 *))
  (check-type now (integer 0 *))
  (let ((games (getf state :games)))
    (when (>= game-index (length games))
      (return-from dashboard-loop-request-game
        (values (copy-list state) nil)))
    (let* ((game (nth game-index games))
           (terminal-mode (getf game :terminal-mode))
           (reboot (dashboard-application-id-p game "reboot"))
           (next (if reboot state (dashboard-loop-cancel-reboot state))))
      (cond
        (terminal-mode
         (setf next (copy-list next)
               (getf next :pending-launch)
               (list :kind :terminal :game-index game-index
                     :mode terminal-mode))
         (values next nil))
        (reboot
         (if (reboot-confirmation-active-p
              (getf next :reboot-deadline) now)
             (progn
               (setf next (copy-list next)
                     (getf next :reboot-deadline) 0
                     (getf next :pending-launch)
                     (list :kind :reboot :game-index game-index))
               (values next nil))
             (progn
               (setf next (copy-list next)
                     (getf next :reboot-deadline)
                     (+ now (dashboard-timing :reboot-confirm-ms)))
               (setf next
                     (dashboard-loop-set-global-status
                      next *dashboard-reboot-confirmation-text*))
               (values next (dashboard-loop-screen-effects)))))
        (t
         (setf next (copy-list next)
               (getf next :pending-launch)
               (list :kind :game :game-index game-index))
         (values next nil))))))

(defun dashboard-loop-apply-settings-plan (state settings plan)
  (let ((next (copy-list state)))
    (setf (getf next :settings) settings)
    (case (getf plan :action)
      (:close
       (setf next (dashboard-loop-set-view next :dashboard)
             next (dashboard-loop-set-global-status next ""))
       (values next (dashboard-loop-screen-effects :back)))
      ((:volume :brightness :keymap)
       (setf (getf next :pending-settings-plan) (copy-tree plan))
       (values next (list (list :settings-action (copy-tree plan)))))
      (:terminal
       (setf (getf next :pending-launch)
             (list :kind :terminal :mode (getf plan :mode)))
       (values next (list (list :cue (getf plan :cue)))))
      (:wifi
       (setf (getf next :wifi) (wifi-open-state (getf next :wifi))
             next (dashboard-loop-set-view next :wifi))
       (values next (dashboard-loop-screen-effects (getf plan :cue))))
      (otherwise (values next nil)))))

(defun dashboard-loop-command-transition (state command layout now)
  (let ((view (getf state :view)))
    (case command
      (:back
       (let ((next (dashboard-loop-cancel-reboot state)))
         (setf next
               (case view
                 (:credits
                  (dashboard-loop-set-global-status
                   (dashboard-loop-set-view next :dashboard) ""))
                 (:wifi
                  (dashboard-loop-set-global-status
                   (dashboard-loop-set-view next :settings)
                   (dashboard-wifi-label :closed)))
                 (:settings
                  (dashboard-loop-set-global-status
                   (dashboard-loop-set-view next :dashboard) ""))
                 (otherwise next)))
         (values next (dashboard-loop-screen-effects :back))))
      (:settings
       (let* ((opening (not (eq view :settings)))
              (next (dashboard-loop-cancel-reboot state))
              (settings (copy-list (getf next :settings))))
         (when opening
           (setf (getf settings :selected) :volume-down))
         (setf (getf next :settings) settings
               next (dashboard-loop-set-view
                     next (if opening :settings :dashboard))
               next (dashboard-loop-set-global-status next ""))
         (values next
                 (dashboard-loop-screen-effects
                  (if opening :confirm :back)))))
      ((:system-previous :system-next)
       (let* ((next (dashboard-loop-cancel-reboot state))
              (dashboard (copy-list (getf next :dashboard)))
              (direction (if (eq command :system-previous) -1 1)))
         (setf (getf dashboard :active-system)
               (dashboard-adjacent-system
                (getf layout :systems) (getf dashboard :active-system)
                direction)
               (getf dashboard :game-position) 0
               (getf next :dashboard) dashboard
               next (dashboard-loop-set-global-status next ""))
         (values next
                 (dashboard-loop-screen-effects
                  (if (minusp direction) :previous :next)))))
      ((:previous :next)
       (let ((next (dashboard-loop-cancel-reboot state)))
         (if (eq view :settings)
             (multiple-value-bind (settings effect)
                 (settings-move-selection (getf next :settings) command)
               (setf (getf next :settings) settings
                     next (dashboard-loop-set-global-status
                           next (getf settings :status)))
               (values next
                       (dashboard-loop-screen-effects (getf effect :cue))))
             (let* ((dashboard (copy-list (getf next :dashboard)))
                    (count (length (getf layout :game-indices)))
                    (position (getf dashboard :game-position))
                    (moved (plusp count)))
               (when moved
                 (setf (getf dashboard :game-position)
                       (if (eq command :previous)
                           (if (zerop position) (1- count) (1- position))
                           (mod (1+ position) count))))
               (setf (getf next :dashboard) dashboard
                     next (dashboard-loop-set-global-status next ""))
               (values next
                       (dashboard-loop-screen-effects
                        (and moved command)))))))
      (:confirm
       (if (eq view :settings)
           (multiple-value-bind (settings plan)
               (settings-controller-transition (getf state :settings) :confirm)
             (dashboard-loop-apply-settings-plan state settings plan))
           (let ((game-index (getf layout :shown-game-index)))
             (if (< game-index (length (getf state :games)))
                 (multiple-value-bind (next effects)
                     (dashboard-loop-request-game state game-index now)
                   (values next (append effects '((:cue :confirm)))))
                 (values (copy-list state) nil)))))
      (otherwise (values (copy-list state) nil)))))

(defun dashboard-reduce-controls (state event)
  (let* ((arguments (rest event))
         (gamepad (or (getf arguments :gamepad-actions) nil))
         (keyboard (or (getf arguments :keyboard-actions) nil))
         (now (getf arguments :now))
         (layout (getf arguments :layout))
         (missing (gensym))
         (quarantine (getf arguments :controller-quarantined-p missing)))
    (when (eq quarantine missing)
      (error "Dashboard controls need explicit audio quarantine state"))
    (check-type now (integer 0 *))
    (check-type layout list)
    (let ((quarantined (not (null quarantine))))
    (multiple-value-bind (actions guard newly-suspended)
        (dashboard-controller-input-actions
         gamepad keyboard (getf state :controller-guard) now
         :controller-quarantined-p quarantined)
      (let* ((next (copy-list state))
             (view (getf state :view))
             (command
               (dashboard-controller-command
                actions (member view '(:wifi :credits)) (eq view :settings)))
             (prefix (and newly-suspended '((:controller-suspended)))))
        (setf (getf next :controller-guard) guard)
        (if command
            (multiple-value-bind (command-state effects)
                (dashboard-loop-command-transition
                 (dashboard-loop-clear-pressed-targets next) command layout now)
              (values command-state
                      (append prefix '((:discard-touch)) effects)))
            (values next prefix)))))))

(defun dashboard-reduce-tick (state event)
  (let ((now (getf (rest event) :now)))
    (check-type now (integer 0 *))
    (multiple-value-bind (guard recovered)
        (dashboard-controller-guard-recover-if-quiet
         (getf state :controller-guard) now)
      (let ((next (copy-list state))
            (effects (and recovered '((:controller-resumed)))))
        (setf (getf next :controller-guard) guard)
        (when (and (plusp (getf next :reboot-deadline))
                   (not (reboot-confirmation-active-p
                         (getf next :reboot-deadline) now)))
          (setf (getf next :reboot-deadline) 0)
          (when (string= (getf (getf next :dashboard) :status)
                         *dashboard-reboot-confirmation-text*)
            (setf next (dashboard-loop-set-global-status next "")
                  effects (append effects
                                  (dashboard-loop-screen-effects)))))
        (values next effects)))))

(defun dashboard-loop-released-reboot-p (state target)
  (and (consp target)
       (eq (first target) :game)
       (let ((index (second target)))
         (and (typep index '(integer 0 *))
              (< index (length (getf state :games)))
              (dashboard-application-id-p
               (nth index (getf state :games)) "reboot")))))

(defun dashboard-reduce-dashboard-touch (state report layout now)
  (destructuring-bind (x y down pressed released) report
    (declare (ignore down))
    (let* ((dashboard (getf state :dashboard))
           (target (dashboard-target-at layout x y))
           (pressed-target
             (if pressed target (getf dashboard :pressed-target))))
      (unless released
        (multiple-value-bind (next-dashboard effect)
            (dashboard-touch-transition dashboard layout report)
          (declare (ignore effect))
          (let ((next (copy-list state)))
            (setf (getf next :dashboard) next-dashboard)
            (return-from dashboard-reduce-dashboard-touch
              (values next nil)))))
      (let ((next state))
        (when (and (plusp (getf state :reboot-deadline))
                   (not (dashboard-loop-released-reboot-p state target)))
          (setf next (dashboard-loop-cancel-reboot next)))
        (cond
          ((and (eq pressed-target :credits) (eq target :credits))
           (let ((next-dashboard (copy-list (getf next :dashboard))))
             (setf (getf next-dashboard :pressed-target) nil
                   next (copy-list next)
                   (getf next :dashboard) next-dashboard
                   (getf next :credits-started-at) now
                   next (dashboard-loop-set-view next :credits)
                   next (dashboard-loop-set-global-status next ""))
             (values next (dashboard-loop-screen-effects :confirm))))
          ((and (eq pressed-target :settings) (eq target :settings))
           (let ((next-dashboard (copy-list (getf next :dashboard)))
                 (settings (copy-list (getf next :settings))))
             (setf (getf next-dashboard :pressed-target) nil
                   (getf settings :selected) :volume-down
                   next (copy-list next)
                   (getf next :dashboard) next-dashboard
                   (getf next :settings) settings
                   next (dashboard-loop-set-view next :settings)
                   next (dashboard-loop-set-global-status next ""))
             (values next (dashboard-loop-screen-effects :confirm))))
          ((and (consp pressed-target) (eq (first pressed-target) :game)
                (equal pressed-target target))
           (let ((next-dashboard (copy-list (getf next :dashboard))))
             (setf (getf next-dashboard :pressed-target) nil
                   next (copy-list next)
                   (getf next :dashboard) next-dashboard)
             (multiple-value-bind (requested effects)
                 (dashboard-loop-request-game next (second target) now)
               (values requested (append effects '((:cue :confirm)))))))
          (t
           (multiple-value-bind (next-dashboard effect)
               (dashboard-touch-transition
                (getf next :dashboard) layout report)
             (setf next (copy-list next)
                   (getf next :dashboard) next-dashboard)
             (values next (dashboard-loop-transition-effects effect)))))))))

(defun dashboard-reduce-settings-touch (state report layout)
  (multiple-value-bind (settings plan)
      (settings-touch-transition (getf state :settings) layout report)
    (if plan
        (dashboard-loop-apply-settings-plan state settings plan)
        (let ((next (copy-list state)))
          (setf (getf next :settings) settings)
          (values next nil)))))

(defun dashboard-loop-wifi-screen-effects (cue)
  (append '((:render))
          (and cue (list (list :cue cue)))
          '((:present))))

(defun dashboard-reduce-wifi-touch (state report layout)
  (destructuring-bind (x y down pressed released) report
    (declare (ignore down))
    (let* ((target (wifi-target-at layout x y))
           (pressed-target
             (if pressed target
                 (getf (getf state :wifi) :pressed-target)))
           (matched-release (and released (equal pressed-target target))))
      (multiple-value-bind (wifi effect)
          (wifi-touch-transition (getf state :wifi) layout report)
        (let ((next (copy-list state)))
          (setf (getf next :wifi) wifi)
          (case (getf effect :action)
            (:close
             (setf next (dashboard-loop-set-view next :settings)
                   next (dashboard-loop-set-global-status
                         next (getf effect :dashboard-status)))
             (values next
                     (dashboard-loop-wifi-screen-effects (getf effect :cue))))
            (:save
             (setf (getf next :pending-wifi-plan) (copy-tree effect))
             (values next (list (list :wifi-action (copy-tree effect)))))
            (otherwise
             (values next
                     (cond
                       (effect
                        (dashboard-loop-wifi-screen-effects
                         (getf effect :cue)))
                       (matched-release '((:present)))
                       (t nil))))))))))

(defun dashboard-reduce-credits-touch (state report layout)
  (multiple-value-bind (credits effect)
      (credits-touch-transition (getf state :credits) layout report)
    (let ((next (copy-list state)))
      (setf (getf next :credits) credits)
      (if (getf effect :close)
          (progn
            (setf next (dashboard-loop-set-view next :dashboard)
                  next (dashboard-loop-set-global-status next ""))
            (values next (dashboard-loop-screen-effects (getf effect :cue))))
          (values next nil)))))

(defun dashboard-reduce-touch (state event)
  (let* ((arguments (rest event))
         (report (getf arguments :report))
         (layout (getf arguments :layout))
         (now (getf arguments :now)))
    (check-type report list)
    (check-type layout list)
    (check-type now (integer 0 *))
    (case (getf state :view)
      (:dashboard (dashboard-reduce-dashboard-touch state report layout now))
      (:settings (dashboard-reduce-settings-touch state report layout))
      (:wifi (dashboard-reduce-wifi-touch state report layout))
      (:credits (dashboard-reduce-credits-touch state report layout))
      (otherwise (error "Unknown dashboard view ~S" (getf state :view))))))

(defun dashboard-reduce-settings-result (state event)
  (let ((plan (getf state :pending-settings-plan)))
    (unless plan
      (error "Dashboard has no pending settings action"))
    (let ((succeeded (not (null (getf (rest event) :succeeded-p)))))
      (multiple-value-bind (settings effect)
          (settings-complete-action (getf state :settings) plan succeeded)
        (let ((next (copy-list state)))
          (setf (getf next :settings) settings
                (getf next :pending-settings-plan) nil
                next (dashboard-loop-set-global-status
                      next (getf settings :status)))
          (case (getf plan :action)
            (:volume
             (cond
               ((not succeeded)
                (values next (dashboard-loop-screen-effects)))
               ((zerop (getf plan :value))
                (values next
                        (append (dashboard-loop-screen-effects)
                                '((:stop-sound)))))
               (t
                (setf (getf next :pending-volume-tone) (copy-tree plan))
                (values next
                        (append (dashboard-loop-screen-effects)
                                (list (list :cue (getf effect :cue)
                                            :report-result t)))))))
            (otherwise
             (values next
                     (dashboard-loop-screen-effects (getf effect :cue))))))))))

(defun dashboard-reduce-volume-tone-result (state event)
  (let ((plan (getf state :pending-volume-tone)))
    (unless plan
      (error "Dashboard has no pending volume tone"))
    (let ((next (copy-list state)))
      (setf (getf next :pending-volume-tone) nil)
      (if (getf (rest event) :succeeded-p)
          (values next nil)
          (let ((settings (copy-list (getf next :settings))))
            (setf (getf settings :status) (getf plan :tone-failure-status)
                  (getf next :settings) settings
                  next (dashboard-loop-set-global-status
                        next (getf settings :status)))
            (values next (dashboard-loop-screen-effects)))))))

(defun dashboard-reduce-wifi-result (state event)
  (let ((plan (getf state :pending-wifi-plan)))
    (unless plan
      (error "Dashboard has no pending Wi-Fi action"))
    (multiple-value-bind (wifi effect)
        (wifi-complete-save
         (getf state :wifi) plan
         (not (null (getf (rest event) :succeeded-p)))
         :failure-status (getf (rest event) :failure-status))
      (let ((next (copy-list state)))
        (setf (getf next :wifi) wifi
              (getf next :pending-wifi-plan) nil)
        (values next
                (dashboard-loop-wifi-screen-effects (getf effect :cue)))))))

(defun dashboard-loop-check-pending-event (state event)
  (let ((expected
          (cond ((getf state :pending-settings-plan) :settings-result)
                ((getf state :pending-volume-tone) :volume-tone-result)
                ((getf state :pending-wifi-plan) :wifi-result))))
    (when (and expected (not (eq (first event) expected)))
      (error "Dashboard expects ~S before ~S" expected (first event)))))

(defun dashboard-reduce (state event)
  (check-type state list)
  (check-type event list)
  (dashboard-loop-check-pending-event state event)
  (case (first event)
    (:controls (dashboard-reduce-controls state event))
    (:settings-result (dashboard-reduce-settings-result state event))
    (:tick (dashboard-reduce-tick state event))
    (:touch (dashboard-reduce-touch state event))
    (:volume-tone-result (dashboard-reduce-volume-tone-result state event))
    (:wifi-result (dashboard-reduce-wifi-result state event))
    (otherwise (error "Unknown dashboard event ~S" event))))

(defun apply-dashboard-touch (games state layout report volume-percent presenter)
  (check-type games list)
  (check-type volume-percent (integer 0 100))
  (check-type presenter function)
  (multiple-value-bind (next effect)
      (dashboard-touch-transition state layout report)
    (if (getf effect :render)
        (let ((next-layout
                (render-dashboard games
                                  (getf next :active-system)
                                  (getf next :game-position)
                                  (getf next :status))))
          (funcall presenter)
          (let ((cue (getf effect :cue)))
            (when cue
              (play-menu-sound cue volume-percent)))
          (values next next-layout effect))
        (values next layout nil))))

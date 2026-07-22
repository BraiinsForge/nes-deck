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
    ;; Until the narrow raster blit arrives, covered games retain a complete
    ;; deterministic fallback card rather than aborting the dashboard frame.
    (unless (draw-dashboard-compact-logo art-x art-y art-size art-size game)
      (draw-dashboard-cartridge art-x art-y art-size art-size color))
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
  (clear-canvas (dashboard-color :background))
  (let ((credits (dashboard-menu-geometry :credits))
        (settings (dashboard-menu-geometry :settings)))
    (apply #'draw-centered-text
           (append credits (list "(c)" 2 (dashboard-color :footer))))
    (apply #'draw-dashboard-settings-fallback settings)
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

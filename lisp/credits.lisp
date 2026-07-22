(in-package #:retrodeck)

(defparameter *dashboard-credits-path* "/mnt/data/nes-deck/menu/credits.tsv")
(defparameter *dashboard-credits-archive-path* "/mnt/data/nes-deck/licenses")

(defparameter *dashboard-credits-labels*
  '(:crawl-title "RETRO DECK"
    :crawl-subtitle "BUILT ON FREE SOFTWARE"
    :archive-title "LICENSE TEXT ARCHIVE"
    :thanks "THANK YOU"
    :static-title "FOSS CREDITS"
    :static-heading "PROJECT / LICENSE"
    :unavailable "CREDITS UNAVAILABLE"))

(defparameter *dashboard-credits-geometry*
  '(:canvas-width 1280 :canvas-height 480
    :maximum-file-bytes 32768 :maximum-projects 64
    :maximum-project-bytes 48 :maximum-role-bytes 64
    :maximum-license-bytes 64
    :text-scale 4 :maximum-line-width 1040 :line-advance 44 :section-gap 28
    :horizon-y 56 :clip-top 72 :fade-invisible-y 104 :fade-opaque-y 210
    :bottom-y 480 :camera-distance 420 :maximum-depth 4000
    :speed-numerator 1 :speed-denominator 20
    :star-count 96 :star-skip-every 7 :star-x-multiplier 193 :star-x-offset 47
    :star-y-multiplier 83 :star-y-offset 29 :star-large-every 11
    :close (1212 12 56 56) :close-radius 12 :close-step 4 :close-pixel-size 4
    :unavailable (80 180 1120 120 3)
    :static-title (20 20 2) :static-heading (20 48 1)
    :static-left-margin 24 :static-top 78 :static-row-advance 22
    :static-rows-per-column 16 :static-column-padding 20
    :static-footer (20 458 1)))

(defparameter *credits-text-mask-cache* (make-hash-table :test #'equal))

(defun dashboard-credits-value (values name description)
  (let ((missing (gensym)))
    (let ((value (getf values name missing)))
      (if (eq value missing)
          (error "Unknown dashboard credits ~A ~S" description name)
          (if (listp value) (copy-list value) value)))))

(defun dashboard-credits-label (name)
  (dashboard-credits-value *dashboard-credits-labels* name "label"))

(defun dashboard-credits-geometry (name)
  (dashboard-credits-value *dashboard-credits-geometry* name "geometry"))

(defun credits-lines (text)
  (loop with start = 0
        while (< start (length text))
        for end = (position #\Newline text :start start)
        collect (subseq text start (or end (length text)))
        do (if end (setf start (1+ end)) (loop-finish))))

(defun credits-tab-fields (line)
  (loop with start = 0
        for end = (position #\Tab line :start start)
        collect (subseq line start (or end (length line)))
        do (if end (setf start (1+ end)) (loop-finish))))

(defun valid-credits-field-p (field maximum)
  (and (plusp (length field))
       (<= (length field) maximum)
       (loop for character across field
             for code = (char-code character)
             always (and (<= #x20 code #x7e) (/= code #x09)))))

(defun load-project-credits (&optional (path *dashboard-credits-path*))
  (let ((name (namestring (pathname path))))
    (unless (and (plusp (length name)) (char= (char name 0) #\/))
      (error "Credits path must be absolute"))
    (let ((contents (read-bounded-regular-file
                     name 1 (dashboard-credits-geometry :maximum-file-bytes))))
      (unless contents
        (error "Cannot read credits ~A" name))
      (let ((seen (make-hash-table :test #'equal))
            (credits nil))
        (loop for raw-line in (credits-lines contents)
              for line-number from 1
              for line = (if (and (plusp (length raw-line))
                                  (char= (char raw-line (1- (length raw-line)))
                                         #\Return))
                             (subseq raw-line 0 (1- (length raw-line)))
                             raw-line)
              unless (or (zerop (length line)) (char= (char line 0) #\#))
                do (let ((fields (credits-tab-fields line)))
                     (unless (and (= (length fields) 3)
                                  (valid-credits-field-p
                                   (first fields)
                                   (dashboard-credits-geometry
                                    :maximum-project-bytes))
                                  (valid-credits-field-p
                                   (second fields)
                                   (dashboard-credits-geometry
                                    :maximum-role-bytes))
                                  (valid-credits-field-p
                                   (third fields)
                                   (dashboard-credits-geometry
                                    :maximum-license-bytes)))
                       (error "Invalid credits row ~D" line-number))
                     (when (gethash (first fields) seen)
                       (error "Duplicate credits project ~A" (first fields)))
                     (setf (gethash (first fields) seen) t)
                     (push (list :project (first fields)
                                 :role (second fields)
                                 :license (third fields))
                           credits)
                     (when (> (length credits)
                              (dashboard-credits-geometry :maximum-projects))
                       (error "Credits contain more than 64 projects"))))
        (unless credits
          (error "Credits contain no projects"))
        (nreverse credits)))))

(defun wrap-credits-text (text)
  (let* ((scale (dashboard-credits-geometry :text-scale))
         (maximum-width (dashboard-credits-geometry :maximum-line-width))
         (maximum-characters
           (floor (+ maximum-width scale)
                  (* +bitmap-glyph-advance+ scale)))
         (remaining (display-ascii text))
         (wrapped nil))
    (loop while (plusp (length remaining)) do
      (setf remaining (string-left-trim " " remaining))
      (when (zerop (length remaining))
        (return))
      (when (<= (bitmap-text-width remaining scale) maximum-width)
        (push remaining wrapped)
        (return))
      (let ((split (position #\Space remaining :from-end t
                             :end (min (length remaining)
                                       (1+ maximum-characters)))))
        (when (or (null split) (zerop split))
          (setf split maximum-characters))
        (push (subseq remaining 0 split) wrapped)
        (setf remaining (subseq remaining split))))
    (nreverse wrapped)))

(defun make-project-credits-crawl (credits)
  (check-type credits list)
  (let ((cursor 0)
        (lines nil)
        (static-lines nil)
        (scale (dashboard-credits-geometry :text-scale)))
    (labels ((append-text (text)
               (dolist (shown (wrap-credits-text text))
                 (push (list :text shown :source-y cursor
                             :source-width (bitmap-text-width shown scale)
                             :source-height (* +bitmap-glyph-height+ scale))
                       lines)
                 (incf cursor (dashboard-credits-geometry :line-advance)))))
      (when credits
        (append-text (dashboard-credits-label :crawl-title))
        (append-text (dashboard-credits-label :crawl-subtitle))
        (incf cursor (dashboard-credits-geometry :section-gap))
        (dolist (credit credits)
          (push (display-ascii
                 (format nil "~A / ~A" (getf credit :project)
                         (getf credit :license)))
                static-lines)
          (append-text (getf credit :project))
          (append-text (getf credit :role))
          (append-text (getf credit :license))
          (incf cursor (dashboard-credits-geometry :section-gap)))
        (append-text (dashboard-credits-label :archive-title))
        (append-text *dashboard-credits-archive-path*)
        (incf cursor (dashboard-credits-geometry :section-gap))
        (append-text (dashboard-credits-label :thanks))))
    (list :lines (nreverse lines)
          :static-lines (nreverse static-lines)
          :content-height cursor)))

(defun clear-credits-text-mask-cache ()
  (let ((cleared (clear-text-mask-cache)))
    (clrhash *credits-text-mask-cache*)
    cleared))

(defun credits-text-mask (text)
  (let ((key (list text (dashboard-credits-geometry :text-scale))))
    (multiple-value-bind (handle present-p)
        (gethash key *credits-text-mask-cache*)
      (if present-p
          handle
          (let ((loaded (load-text-mask text (second key))))
            (unless loaded
              (error "Cannot load projected credits text ~S" text))
            (setf (gethash key *credits-text-mask-cache*) loaded))))))

(defun prepare-project-credits-crawl (crawl)
  (check-type crawl list)
  (dolist (line (getf crawl :lines) crawl)
    (credits-text-mask (getf line :text))))

(defun rgb565-display-color (color)
  (labels ((expand (value maximum) (floor (* value 255) maximum)))
    (let ((red (expand (ash (ldb (byte 8 16) color) -3) 31))
          (green (expand (ash (ldb (byte 8 8) color) -2) 63))
          (blue (expand (ash (ldb (byte 8 0) color) -3) 31)))
      (logior (ash red 16) (ash green 8) blue))))

(defun draw-credits-starfield (color)
  (let ((width (dashboard-credits-geometry :canvas-width))
        (height (dashboard-credits-geometry :canvas-height)))
    (dotimes (index (dashboard-credits-geometry :star-count) t)
      (unless (zerop (mod index (dashboard-credits-geometry :star-skip-every)))
        (let ((x (mod (+ (* index (dashboard-credits-geometry
                                  :star-x-multiplier))
                         (dashboard-credits-geometry :star-x-offset))
                      width))
              (y (mod (+ (* index (dashboard-credits-geometry
                                  :star-y-multiplier))
                         (dashboard-credits-geometry :star-y-offset))
                      height))
              (size (if (zerop (mod index (dashboard-credits-geometry
                                           :star-large-every)))
                        2 1)))
          (fill-canvas-rect x y size size color))))))

(defun draw-credits-close (color)
  (destructuring-bind (x y width height)
      (dashboard-credits-geometry :close)
    (let ((center-x (+ x (floor width 2)))
          (center-y (+ y (floor height 2)))
          (radius (dashboard-credits-geometry :close-radius))
          (step (dashboard-credits-geometry :close-step))
          (size (dashboard-credits-geometry :close-pixel-size)))
      (loop for offset from (- radius) to radius by step do
        (fill-canvas-rect (+ center-x offset) (+ center-y offset)
                          size size color)
        (fill-canvas-rect (+ center-x offset) (- center-y offset)
                          size size color)))))

(defun draw-static-credits (crawl accent text muted)
  (destructuring-bind (x y scale)
      (dashboard-credits-geometry :static-title)
    (draw-text x y (dashboard-credits-label :static-title) scale accent))
  (destructuring-bind (x y scale)
      (dashboard-credits-geometry :static-heading)
    (draw-text x y (dashboard-credits-label :static-heading) scale muted))
  (let* ((lines (getf crawl :static-lines))
         (rows (dashboard-credits-geometry :static-rows-per-column))
         (columns (max 1 (ceiling (length lines) rows)))
         (left (dashboard-credits-geometry :static-left-margin))
         (column-width (floor (- (dashboard-credits-geometry :canvas-width)
                                 (* left 2))
                              columns)))
    (loop for line in lines
          for index from 0
          for column = (floor index rows)
          for row = (mod index rows)
          do (draw-text (+ left (* column column-width))
                        (+ (dashboard-credits-geometry :static-top)
                           (* row (dashboard-credits-geometry
                                   :static-row-advance)))
                        (fit-text-width
                         line
                         (- column-width
                            (dashboard-credits-geometry :static-column-padding))
                         1)
                        1 text)))
  (destructuring-bind (x y scale)
      (dashboard-credits-geometry :static-footer)
    (draw-text x y *dashboard-credits-archive-path* scale muted)))

(defun draw-animated-credits (crawl elapsed-ms accent)
  (let* ((maximum-depth (dashboard-credits-geometry :maximum-depth))
         (cycle (+ (getf crawl :content-height) maximum-depth)))
    (unless (configure-text-projection
             elapsed-ms
             (dashboard-credits-geometry :speed-numerator)
             (dashboard-credits-geometry :speed-denominator)
             cycle
             (dashboard-credits-geometry :camera-distance)
             maximum-depth
             (dashboard-credits-geometry :horizon-y)
             (dashboard-credits-geometry :clip-top)
             (dashboard-credits-geometry :fade-invisible-y)
             (dashboard-credits-geometry :fade-opaque-y)
             (dashboard-credits-geometry :bottom-y)
             accent)
      (error "Cannot configure projected credits text"))
    (dolist (line (getf crawl :lines) t)
      (unless (draw-projected-text (credits-text-mask (getf line :text))
                                   (getf line :source-y))
        (error "Cannot draw projected credits text ~S" (getf line :text))))))

(defun render-project-credits (crawl reduced-motion elapsed-ms)
  (check-type crawl list)
  (check-type reduced-motion boolean)
  (check-type elapsed-ms (integer -9223372036854775808 9223372036854775807))
  (let ((background (rgb565-display-color (dashboard-color :background)))
        (accent (rgb565-display-color (dashboard-color :title)))
        (text (rgb565-display-color (dashboard-color :text)))
        (muted (rgb565-display-color (dashboard-color :muted)))
        (close (dashboard-credits-geometry :close)))
    (clear-canvas background)
    (unless reduced-motion
      (draw-credits-starfield muted))
    (cond ((or (null (getf crawl :lines))
               (not (plusp (getf crawl :content-height))))
           (destructuring-bind (x y width height scale)
               (dashboard-credits-geometry :unavailable)
             (draw-centered-text x y width height
                                 (dashboard-credits-label :unavailable)
                                 scale text)))
          (reduced-motion
           (draw-static-credits crawl accent text muted))
          (t
           (draw-animated-credits crawl (max 0 elapsed-ms) accent)))
    (draw-credits-close muted)
    (list :close close)))

(defun credits-bounds-contains-p (bounds x y)
  (destructuring-bind (left top width height) bounds
    (and (<= left x) (< x (+ left width))
         (<= top y) (< y (+ top height)))))

(defun credits-target-at (layout x y)
  (check-type layout list)
  (if (credits-bounds-contains-p (getf layout :close) x y) :close nil))

(defun credits-initial-state ()
  (list :pressed-target nil))

(defun credits-touch-transition (state layout report)
  (destructuring-bind (x y down pressed released) report
    (declare (ignore down))
    (let* ((next (copy-list state))
           (target (credits-target-at layout x y))
           (effect nil))
      (when pressed
        (setf (getf next :pressed-target) target))
      (when released
        (let ((pressed-target (getf next :pressed-target)))
          (setf (getf next :pressed-target) nil)
          (when (eq pressed-target target)
            (when (eq target :close)
              (setf effect '(:close t :cue :back))))))
      (values next effect))))

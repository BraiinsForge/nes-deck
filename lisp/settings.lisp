(in-package #:retrodeck)

(defparameter *dashboard-settings-labels*
  '(:active-wifi "ACTIVE WIFI"
    :not-connected "NOT CONNECTED"
    :wlan "WLAN0"
    :wireguard "WIREGUARD"
    :no-address "NO ADDRESS"
    :auto-wifi-prefix "AUTO WIFI: "
    :wifi "WIFI"
    :wifi-settings "SETTINGS"
    :volume "VOLUME"
    :brightness "BRIGHTNESS"
    :terminal "TERMINAL"
    :keys "KEYS"
    :off "OFF"
    :english-short "EN"
    :czech-short "CZ"
    :us-ansi "US ANSI"
    :czech "CZECH"
    :volume-muted-status "GAME VOLUME MUTED"
    :volume-status-prefix "GAME VOLUME "
    :volume-error-status "VOLUME STATE ERROR"
    :volume-tone-error-status "VOLUME SAVED; CONFIRMATION TONE FAILED"
    :brightness-status-prefix "BRIGHTNESS "
    :brightness-error-status "BRIGHTNESS ERROR - CHECK LOG"
    :keymap-us-status "TERMINAL KEYS: US ANSI"
    :keymap-czech-status "TERMINAL KEYS: CZECH"
    :keymap-error-status "KEYMAP STATE ERROR"))

(defparameter *dashboard-settings-geometry*
  '(:canvas-width 1280 :canvas-height 480
    :close (1212 12 56 56)
    :wifi (926 20 262 108)
    :volume-down (108 208 104 104)
    :volume-up (228 208 104 104)
    :brightness-down (438 208 104 104)
    :brightness-up (558 208 104 104)
    :terminal (792 208 112 104)
    :keymap (1036 208 112 104)
    :active-wifi-label (64 22 1)
    :active-wifi-value (64 44 300 3)
    :wlan-label (392 22 1)
    :wlan-value (392 44 2)
    :wireguard-label (620 22 1)
    :wireguard-value (620 44 2)
    :selector (64 88 790 1)
    :wifi-icon (12 24 54 54)
    :wifi-title (78 28 3)
    :wifi-subtitle (78 64 2)
    :volume-value (82 328 276 34 3)
    :volume-label (82 366 276 28 2)
    :brightness-value (412 328 276 34 3)
    :brightness-label (412 366 276 28 2)
    :terminal-value (750 328 196 34 3)
    :terminal-label (750 366 196 28 2)
    :keymap-value (994 328 196 34 3)
    :keymap-label (994 366 196 28 2)
    :status (12 440 1256 28 2 1)
    :close-radius 16 :close-step 4 :close-pixel-size 4))

(defparameter *dashboard-settings-paths*
  '(:volume-state "/mnt/data/nes-deck/state/menu-volume.state"
    :brightness "/sys/class/backlight/display-bl/brightness"
    :brightness-maximum "/sys/class/backlight/display-bl/max_brightness"
    :brightness-state "/mnt/data/nes-deck/state/menu-brightness.state"
    :keymap-state "/mnt/data/nes-deck/state/terminal-keymap.state"))

(defparameter *dashboard-settings-selection-order*
  '(:volume-down :volume-up :brightness-down :brightness-up
    :terminal :keymap :wifi))

(defun dashboard-settings-value (values name description)
  (let ((missing (gensym)))
    (let ((value (getf values name missing)))
      (if (eq value missing)
          (error "Unknown dashboard settings ~A ~S" description name)
          (if (listp value) (copy-list value) value)))))

(defun dashboard-settings-label (name)
  (dashboard-settings-value *dashboard-settings-labels* name "label"))

(defun dashboard-settings-geometry (name)
  (dashboard-settings-value *dashboard-settings-geometry* name "geometry"))

(defun dashboard-settings-path (name)
  (dashboard-settings-value *dashboard-settings-paths* name "path"))

(defun draw-settings-control (bounds selected)
  (destructuring-bind (x y width height) bounds
    (draw-pixel-panel
     x y width height
     (dashboard-color (if selected :active :control-surface))
     (dashboard-color (if selected :accent :control-border)))))

(defun draw-settings-close-icon (bounds color)
  (destructuring-bind (x y width height) bounds
    (let ((center-x (+ x (floor width 2)))
          (center-y (+ y (floor height 2)))
          (radius (dashboard-settings-geometry :close-radius))
          (step (dashboard-settings-geometry :close-step))
          (size (dashboard-settings-geometry :close-pixel-size)))
      (loop for offset from (- radius) to radius by step do
        (fill-canvas-rect (+ center-x offset) (+ center-y offset)
                          size size color)
        (fill-canvas-rect (+ center-x offset) (- center-y offset)
                          size size color)))))

(defun draw-settings-wifi-icon (bounds color)
  (destructuring-bind (x y width height) bounds
    (declare (ignore height))
    (let ((center-x (+ x (floor width 2)))
          (top (+ y 5)))
      (dolist (rect '((-6 0 12 5) (-12 5 6 5) (6 5 6 5)
                      (-18 10 6 5) (12 10 6 5) (-6 22 12 5)
                      (-12 27 6 5) (6 27 6 5) (-3 36 6 6))
               t)
        (destructuring-bind (offset-x offset-y block-width block-height) rect
          (fill-canvas-rect (+ center-x offset-x) (+ top offset-y)
                            block-width block-height color))))))

(defun draw-settings-speaker-icon (bounds loud color)
  (destructuring-bind (left top width height) bounds
    (declare (ignore width))
    (let ((x (+ left 24))
          (y (+ top (floor height 2))))
      (dolist (rect '((0 -12 12 24) (12 -20 12 40) (24 -28 8 56)
                      (40 -16 4 32) (44 -12 4 24))
               t)
        (destructuring-bind (offset-x offset-y block-width block-height) rect
          (fill-canvas-rect (+ x offset-x) (+ y offset-y)
                            block-width block-height color)))
      (when loud
        (fill-canvas-rect (+ x 56) (- y 24) 4 48 color)
        (fill-canvas-rect (+ x 60) (- y 16) 4 32 color)))))

(defun draw-settings-sun-icon (bounds bright color)
  (destructuring-bind (x y width height) bounds
    (let* ((center-x (+ x (floor width 2)))
           (center-y (+ y (floor height 2)))
           (half (if bright 16 12))
           (reach (if bright 34 28)))
      (fill-pixel-cut-rect (- center-x half) (- center-y half)
                           (* half 2) (* half 2) 4 color)
      (fill-canvas-rect (- center-x 3) (- center-y reach) 6 10 color)
      (fill-canvas-rect (- center-x 3) (+ center-y reach -10) 6 10 color)
      (fill-canvas-rect (- center-x reach) (- center-y 3) 10 6 color)
      (fill-canvas-rect (+ center-x reach -10) (- center-y 3) 10 6 color)
      (when bright
        (dolist (offset '((-25 -25) (18 -25) (-25 18) (18 18)) t)
          (fill-canvas-rect (+ center-x (first offset))
                            (+ center-y (second offset)) 7 7 color))))))

(defun draw-settings-terminal-icon (bounds color)
  (destructuring-bind (x y width height) bounds
    (let* ((icon-height 44)
           (icon-top (+ y (floor (- height icon-height) 2)))
           (screen-x (+ x (floor (- width 46) 2))))
      (stroke-canvas-rect screen-x icon-top 46 34 3 color)
      (fill-canvas-rect (+ x (floor width 2) -3) (+ icon-top 34) 6 7 color)
      (fill-canvas-rect (+ x 24) (+ icon-top 41) (- width 48) 3 color)
      (draw-text (+ screen-x 7) (+ icon-top 9) ">_" 2 color))))

(defun settings-network-text (network key fallback)
  (let ((value (getf network key)))
    (if (and (stringp value) (plusp (length value))) value fallback)))

(defun render-dashboard-settings
    (volume brightness keymap selected status network)
  (check-type volume (integer 0 100))
  (check-type brightness (integer 0 100))
  (check-type keymap string)
  (check-type status string)
  (check-type network list)
  (let* ((close (dashboard-settings-geometry :close))
         (wifi (dashboard-settings-geometry :wifi))
         (volume-down (dashboard-settings-geometry :volume-down))
         (volume-up (dashboard-settings-geometry :volume-up))
         (brightness-down (dashboard-settings-geometry :brightness-down))
         (brightness-up (dashboard-settings-geometry :brightness-up))
         (terminal (dashboard-settings-geometry :terminal))
         (keymap-bounds (dashboard-settings-geometry :keymap))
         (text (dashboard-color :text))
         (muted (dashboard-color :muted)))
    (clear-canvas (dashboard-color :background))
    (draw-settings-close-icon close text)
    (destructuring-bind (x y scale)
        (dashboard-settings-geometry :active-wifi-label)
      (draw-text x y (dashboard-settings-label :active-wifi) scale muted))
    (destructuring-bind (x y maximum-width scale)
        (dashboard-settings-geometry :active-wifi-value)
      (draw-text x y
                 (fit-text-width
                  (settings-network-text
                   network :ssid (dashboard-settings-label :not-connected))
                  maximum-width scale)
                 scale text))
    (destructuring-bind (x y scale)
        (dashboard-settings-geometry :wlan-label)
      (draw-text x y (dashboard-settings-label :wlan) scale muted))
    (destructuring-bind (x y scale)
        (dashboard-settings-geometry :wlan-value)
      (draw-text x y
                 (settings-network-text
                  network :wlan-ipv4 (dashboard-settings-label :no-address))
                 scale text))
    (destructuring-bind (x y scale)
        (dashboard-settings-geometry :wireguard-label)
      (draw-text x y (dashboard-settings-label :wireguard) scale muted))
    (destructuring-bind (x y scale)
        (dashboard-settings-geometry :wireguard-value)
      (draw-text x y
                 (settings-network-text
                  network :wireguard-ipv4
                  (dashboard-settings-label :no-address))
                 scale text))
    (destructuring-bind (x y maximum-width scale)
        (dashboard-settings-geometry :selector)
      (draw-text x y
                 (fit-text-width
                  (concatenate 'string
                               (dashboard-settings-label :auto-wifi-prefix)
                               (or (getf network :selector) ""))
                  maximum-width scale)
                 scale (dashboard-color :footer)))
    (draw-settings-control wifi (eq selected :wifi))
    (destructuring-bind (offset-x offset-y width height)
        (dashboard-settings-geometry :wifi-icon)
      (draw-settings-wifi-icon
       (list (+ (first wifi) offset-x) (+ (second wifi) offset-y) width height)
       text))
    (destructuring-bind (offset-x offset-y scale)
        (dashboard-settings-geometry :wifi-title)
      (draw-text (+ (first wifi) offset-x) (+ (second wifi) offset-y)
                 (dashboard-settings-label :wifi) scale text))
    (destructuring-bind (offset-x offset-y scale)
        (dashboard-settings-geometry :wifi-subtitle)
      (draw-text (+ (first wifi) offset-x) (+ (second wifi) offset-y)
                 (dashboard-settings-label :wifi-settings) scale muted))
    (draw-settings-control volume-down (eq selected :volume-down))
    (draw-settings-control volume-up (eq selected :volume-up))
    (draw-settings-speaker-icon volume-down nil text)
    (draw-settings-speaker-icon volume-up t text)
    (draw-settings-control brightness-down (eq selected :brightness-down))
    (draw-settings-control brightness-up (eq selected :brightness-up))
    (draw-settings-sun-icon brightness-down nil text)
    (draw-settings-sun-icon brightness-up t text)
    (draw-settings-control terminal (eq selected :terminal))
    (draw-settings-terminal-icon terminal text)
    (draw-settings-control keymap-bounds (eq selected :keymap))
    (apply #'draw-centered-text
           (append keymap-bounds
                   (list (dashboard-settings-label
                          (if (string= keymap "cz")
                              :czech-short :english-short))
                         4 text)))
    (flet ((centered (geometry value color)
             (destructuring-bind (x y width height scale)
                 (dashboard-settings-geometry geometry)
               (draw-centered-text x y width height value scale color))))
      (centered :volume-value
                (if (zerop volume)
                    (dashboard-settings-label :off)
                    (format nil "~D" volume))
                text)
      (centered :volume-label (dashboard-settings-label :volume) muted)
      (centered :brightness-value (format nil "~D" brightness) text)
      (centered :brightness-label (dashboard-settings-label :brightness) muted)
      (centered :terminal-value (dashboard-settings-label :terminal) text)
      (centered :terminal-label *dashboard-terminal-login-shell* muted)
      (centered :keymap-value (dashboard-settings-label :keys) text)
      (centered :keymap-label
                (dashboard-settings-label
                 (if (string= keymap "cz") :czech :us-ansi))
                muted))
    (when (plusp (length status))
      (destructuring-bind (x y width height preferred minimum)
          (dashboard-settings-geometry :status)
        (draw-centered-text x y width height status
                            (fit-text-scale status width preferred minimum)
                            (dashboard-color :footer))))
    (list :close close :wifi wifi
          :volume-down volume-down :volume-up volume-up
          :brightness-down brightness-down :brightness-up brightness-up
          :terminal terminal :keymap keymap-bounds)))

(defun settings-bounds-contains-p (bounds x y)
  (destructuring-bind (left top width height) bounds
    (and (<= left x) (< x (+ left width))
         (<= top y) (< y (+ top height)))))

(defun settings-target-at (layout x y)
  (check-type layout list)
  (loop for target in '(:close :volume-down :volume-up
                         :brightness-down :brightness-up
                         :terminal :keymap :wifi)
        when (settings-bounds-contains-p (getf layout target) x y)
          return target))

(defun dashboard-environment-value (name)
  (check-type name string)
  (dolist (entry '(("EXT" "GETENV") ("SB-EXT" "POSIX-GETENV"))
           (error "No Common Lisp environment reader is available"))
    (let ((package (find-package (first entry))))
      (when package
        (let ((symbol (find-symbol (second entry) package)))
          (when (and symbol (fboundp symbol))
            (let ((value (funcall symbol name)))
              (unless (or (null value) (stringp value))
                (error "Environment value ~A is not a string" name))
              (return-from dashboard-environment-value value))))))))

(defun parse-dashboard-inherited-volume (text)
  (check-type *dashboard-volume-default* (integer 0 100))
  (when (null text)
    (return-from parse-dashboard-inherited-volume
      *dashboard-volume-default*))
  (check-type text string)
  (when (zerop (length text))
    (error "RETRO_DECK_VOLUME_PERCENT is empty; expected 0 through 100"))
  (flet ((invalid ()
           (error "RETRO_DECK_VOLUME_PERCENT must be an integer from 0 through 100")))
    (let ((value 0))
      (dotimes (index (length text) value)
        (let ((character (char text index)))
          (unless (char<= #\0 character #\9)
            (invalid))
          (setf value (+ (* value 10) (- (char-code character)
                                         (char-code #\0))))
          (when (> value 100)
            (invalid)))))))

(defun dashboard-inherited-volume ()
  (parse-dashboard-inherited-volume
   (dashboard-environment-value "RETRO_DECK_VOLUME_PERCENT")))

(defun dashboard-volume-state-text (volume)
  (check-type volume (integer 0 100))
  (coerce (format nil "~D~%" volume) 'base-string))

(defun parse-dashboard-volume-state (text)
  (check-type text string)
  (flet ((invalid ()
           (error "volume state must contain a canonical integer from 0 through 100 followed by a newline")))
    (let ((length (length text)))
      (when (< length 2)
        (invalid))
      (let ((digits (1- length)))
        (unless (char= (char text digits) #\Newline)
          (invalid))
        (when (and (> digits 1) (char= (char text 0) #\0))
          (invalid))
        (let ((value 0))
          (dotimes (index digits value)
            (let ((character (char text index)))
              (unless (char<= #\0 character #\9)
                (invalid))
              (setf value (+ (* value 10) (- (char-code character)
                                             (char-code #\0))))
              (when (> value 100)
                (invalid)))))))))

(defun save-dashboard-volume-state (path volume)
  (check-type path string)
  (write-native-state-file path (dashboard-volume-state-text volume)))

(defun load-dashboard-volume-state (path default-volume)
  (check-type path string)
  (check-type default-volume (integer 0 100))
  (multiple-value-bind (text present-p) (read-native-state-file path)
    (let ((volume
            (cond
              ((not present-p) default-volume)
              ((string= text (format nil "on~%")) default-volume)
              ((string= text (format nil "off~%")) 0)
              (t (return-from load-dashboard-volume-state
                   (parse-dashboard-volume-state text))))))
      (unless (save-dashboard-volume-state path volume)
        (error "cannot save volume state ~A" path))
      volume)))

(defun dashboard-keymap-state-text (keymap)
  (check-type keymap string)
  (unless (member keymap '("us" "cz") :test #'string=)
    (error "terminal keymap must be 'us' or 'cz'"))
  (coerce (format nil "~A~%" keymap) 'base-string))

(defun parse-dashboard-keymap-state (text)
  (check-type text string)
  (cond
    ((string= text (format nil "us~%")) "us")
    ((string= text (format nil "cz~%")) "cz")
    (t (error "terminal keymap state must contain exactly 'us\\n' or 'cz\\n'"))))

(defun save-dashboard-keymap-state (path keymap)
  (check-type path string)
  (write-native-state-file path (dashboard-keymap-state-text keymap)))

(defun load-dashboard-keymap-state (path)
  (check-type path string)
  (multiple-value-bind (text present-p) (read-native-state-file path)
    (when present-p
      (return-from load-dashboard-keymap-state
        (parse-dashboard-keymap-state text)))
    (unless (save-dashboard-keymap-state path "us")
      (error "cannot save terminal keymap state ~A" path))
    "us"))

(defun settings-volume-after-target (target volume last-audible-volume)
  (check-type volume (integer 0 100))
  (check-type last-audible-volume (integer 0 100))
  (let ((restore (if (zerop last-audible-volume)
                     *dashboard-volume-step*
                     (min 100 last-audible-volume))))
    (case target
      (:volume-up (if (zerop volume)
                      restore
                      (min 100 (+ volume *dashboard-volume-step*))))
      (:volume-down (if (> volume *dashboard-volume-step*)
                        (- volume *dashboard-volume-step*)
                        0))
      (otherwise volume))))

(defun settings-brightness-after-target (target brightness)
  (check-type brightness (integer 0 100))
  (case target
    (:brightness-up (min 100 (+ brightness *dashboard-brightness-step*)))
    (:brightness-down
     (if (> brightness *dashboard-brightness-minimum*)
         (max *dashboard-brightness-minimum*
              (- brightness *dashboard-brightness-step*))
         *dashboard-brightness-minimum*))
    (otherwise brightness)))

(defun settings-initial-state (&key (volume *dashboard-volume-default*)
                                    last-audible-volume
                                    (brightness 100) (keymap "us"))
  (check-type volume (integer 0 100))
  (check-type brightness (integer 0 100))
  (unless (member keymap '("us" "cz") :test #'string=)
    (error "Unknown terminal keymap ~S" keymap))
  (let ((last (or last-audible-volume
                  (if (plusp volume) volume
                      (if (plusp *dashboard-volume-default*)
                          *dashboard-volume-default*
                          *dashboard-volume-step*)))))
    (check-type last (integer 0 100))
    (list :open t :volume volume :last-audible-volume last
          :brightness brightness :keymap keymap
          :selected :volume-down :pressed-target nil :status "")))

(defun settings-activation-plan (state target)
  (case target
    (:close
     (list :action :close :success-status "" :cue :back))
    ((:volume-down :volume-up)
     (let ((value (settings-volume-after-target
                   target (getf state :volume)
                   (getf state :last-audible-volume))))
       (list :action :volume :path (dashboard-settings-path :volume-state)
             :value value
             :success-status
             (if (zerop value)
                 (dashboard-settings-label :volume-muted-status)
                 (format nil "~A~D%"
                         (dashboard-settings-label :volume-status-prefix)
                         value))
             :failure-status (dashboard-settings-label :volume-error-status)
             :tone-failure-status
             (dashboard-settings-label :volume-tone-error-status)
             :success-effect
             (if (zerop value) '(:stop-sound t) '(:cue :volume)))))
    ((:brightness-down :brightness-up)
     (let ((value (settings-brightness-after-target
                   target (getf state :brightness))))
       (list :action :brightness
             :device-path (dashboard-settings-path :brightness)
             :maximum-path (dashboard-settings-path :brightness-maximum)
             :state-path (dashboard-settings-path :brightness-state)
             :value value
             :success-status
             (format nil "~A~D%"
                     (dashboard-settings-label :brightness-status-prefix)
                     value)
             :failure-status
             (dashboard-settings-label :brightness-error-status)
             :cue (if (eq target :brightness-down) :previous :next))))
    (:terminal
     (list :action :terminal :mode "shell" :cue :confirm))
    (:keymap
     (let ((value (if (string= (getf state :keymap) "cz") "us" "cz")))
       (list :action :keymap :path (dashboard-settings-path :keymap-state)
             :value value
             :success-status
             (dashboard-settings-label
              (if (string= value "cz")
                  :keymap-czech-status :keymap-us-status))
             :failure-status (dashboard-settings-label :keymap-error-status)
             :cue :confirm)))
    (:wifi
     (list :action :wifi :cue :confirm))))

(defun settings-complete-action (state plan succeeded-p
                                 &key (tone-succeeded-p t))
  (let ((next (copy-list state))
        (effect (and (getf plan :cue) (list :cue (getf plan :cue)))))
    (case (getf plan :action)
      (:close
       (setf (getf next :open) nil
             (getf next :status) ""))
      (:volume
       (if succeeded-p
           (let ((value (getf plan :value)))
             (setf effect (copy-list (getf plan :success-effect))
                   (getf next :volume) value
                   (getf next :status)
                   (if (and (plusp value) (not tone-succeeded-p))
                       (getf plan :tone-failure-status)
                       (getf plan :success-status)))
             (when (plusp value)
               (setf (getf next :last-audible-volume) value)))
           (setf effect nil
                 (getf next :status) (getf plan :failure-status))))
      (:brightness
       (if succeeded-p
           (setf (getf next :brightness) (getf plan :value)
                 (getf next :status) (getf plan :success-status))
           (setf (getf next :status) (getf plan :failure-status))))
      (:keymap
       (if succeeded-p
           (setf (getf next :keymap) (getf plan :value)
                 (getf next :status) (getf plan :success-status))
           (setf (getf next :status) (getf plan :failure-status)))))
    (values next effect)))

(defun settings-activate-target (state target)
  (let ((next (copy-list state)))
    (setf (getf next :selected) target)
    (values next (settings-activation-plan next target))))

(defun settings-move-selection (state direction)
  (let* ((next (copy-list state))
         (order *dashboard-settings-selection-order*)
         (position (position (getf state :selected) order))
         (next-position
           (ecase direction
             (:previous (if (or (null position) (zerop position))
                            (1- (length order))
                            (1- position)))
             (:next (if position
                        (mod (1+ position) (length order))
                        0)))))
    (setf (getf next :selected) (nth next-position order)
          (getf next :status) "")
    (values next (list :cue direction))))

(defun settings-controller-transition (state command)
  (case command
    (:back (values (copy-list state) (settings-activation-plan state :close)))
    ((:previous :next) (settings-move-selection state command))
    (:confirm (settings-activate-target state (getf state :selected)))
    (otherwise (values (copy-list state) nil))))

(defun settings-touch-transition (state layout report)
  (destructuring-bind (x y down pressed released) report
    (declare (ignore down))
    (let* ((next (copy-list state))
           (target (settings-target-at layout x y)))
      (when pressed
        (setf (getf next :pressed-target) target))
      (if released
          (let ((pressed-target (getf next :pressed-target)))
            (setf (getf next :pressed-target) nil)
            (if (and target (eq pressed-target target))
                (settings-activate-target next target)
                (values next nil)))
          (values next nil)))))

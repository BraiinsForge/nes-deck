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

(defpackage #:retrodeck
  (:use #:cl)
  (:import-from #:retrodeck.native
                #:abi-version
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
                #:wayland-size)
  (:export #:*dashboard-brightness-minimum*
           #:*dashboard-brightness-step*
           #:*dashboard-built-in-applications*
           #:*dashboard-controller-burst-limit*
           #:*dashboard-gamepad-controls*
           #:*dashboard-keyboard-controls*
           #:*dashboard-loop-labels*
           #:*dashboard-cover-directory*
           #:*dashboard-credits-archive-path*
           #:*dashboard-credits-geometry*
           #:*dashboard-credits-labels*
           #:*dashboard-credits-path*
           #:*dashboard-executables*
           #:*dashboard-menu-geometry*
           #:*dashboard-palette*
           #:*dashboard-reboot-confirmation-text*
           #:*dashboard-reduced-motion-environment*
           #:*dashboard-settings-geometry*
           #:*dashboard-settings-icon-path*
           #:*dashboard-settings-labels*
           #:*dashboard-settings-paths*
           #:*dashboard-settings-selection-order*
           #:*dashboard-systems*
           #:*dashboard-wifi-geometry*
           #:*dashboard-wifi-key-rows*
           #:*dashboard-wifi-labels*
           #:*dashboard-wifi-limits*
           #:*dashboard-wifi-paths*
           #:*dashboard-terminal-login-shell*
           #:*dashboard-timings*
           #:*dashboard-volume-default*
           #:*dashboard-volume-step*
           #:*menu-sound-cues*
           #:*menu-sound-input-tail-ms*
           #:apply-dashboard-touch
           #:bitmap-text-width
           #:canvas-rgb565-hash
           #:clear-canvas
           #:clear-dashboard-raster-cache
           #:clear-credits-text-mask-cache
           #:clear-text-mask-cache
           #:collect-dashboard-control-actions
           #:configure-text-projection
           #:close-evdev-controls
           #:close-evdev-touch
           #:close-fbdev
           #:close-wayland
           #:current-fbdev-size
           #:current-wayland-size
           #:credits-initial-state
           #:credits-target-at
           #:credits-touch-transition
           #:dashboard-application
           #:dashboard-color
           #:dashboard-control-actions
           #:dashboard-controller-command
           #:dashboard-controller-guard-accept-edge
           #:dashboard-controller-guard-initial-state
           #:dashboard-controller-guard-recover-if-quiet
           #:dashboard-controller-input-actions
           #:dashboard-controller-scan-due-p
           #:dashboard-credits-geometry
           #:dashboard-credits-label
           #:dashboard-executable
           #:dashboard-settings-geometry
           #:dashboard-settings-label
           #:dashboard-settings-path
           #:dashboard-wifi-geometry
           #:dashboard-wifi-key-rows
           #:dashboard-wifi-label
           #:dashboard-wifi-limit
           #:dashboard-wifi-path
           #:dashboard-initial-state
           #:dashboard-launch-plan
           #:dashboard-loop-begin-iteration
           #:dashboard-loop-dispatch-input
           #:dashboard-loop-initial-state
           #:dashboard-loop-label
           #:dashboard-loop-poll-timeout
           #:dashboard-loop-step
           #:dashboard-reduce
           #:dashboard-runtime-begin-iteration
           #:dashboard-runtime-controller-quarantined-p
           #:dashboard-runtime-dispatch-input
           #:dashboard-runtime-initialize
           #:dashboard-runtime-running-p
           #:dashboard-runtime-shutdown
           #:dashboard-menu-geometry
           #:dashboard-system-label
           #:dashboard-target-at
           #:dashboard-terminal-result-status
           #:dashboard-terminal-starting-status
           #:dashboard-terminal-title
           #:dashboard-timing
           #:dashboard-touch-transition
           #:dispatch-evdev-controls
           #:dispatch-evdev-touch
           #:dispatch-wayland
           #:display-ascii
           #:draw-canvas-glyph
           #:draw-canvas-raster
           #:draw-projected-text
           #:draw-centered-text
           #:draw-pixel-panel
           #:draw-text
           #:fill-canvas-rect
           #:fill-pixel-cut-rect
           #:finish-menu-sound
           #:fit-text-scale
           #:fit-text-width
           #:load-cover-raster
           #:load-png-raster
           #:load-project-credits
           #:load-text-mask
           #:main
           #:make-dashboard-runtime
           #:make-project-credits-crawl
           #:menu-sound-blocks-input-p
           #:menu-sound-duration-ms
           #:menu-sound-notes
           #:next-evdev-control
           #:next-evdev-touch
           #:next-wayland-touch
           #:open-evdev-touch
           #:open-fbdev
           #:open-wayland-widget
           #:play-menu-sound
           #:prepare-dashboard-rasters
           #:prepare-project-credits-crawl
           #:present-fbdev-canvas
           #:present-fbdev-solid
           #:present-wayland-canvas
           #:present-wayland-solid
           #:read-bounded-regular-file
           #:reboot-confirmation-active-p
           #:scan-evdev-controls
           #:run-dashboard-terminal
           #:render-dashboard
           #:render-dashboard-loop-state
           #:render-dashboard-settings
           #:render-dashboard-wifi
           #:render-project-credits
           #:settings-activation-plan
           #:settings-brightness-after-target
           #:settings-complete-action
           #:settings-controller-transition
           #:settings-initial-state
           #:settings-move-selection
           #:settings-target-at
           #:settings-touch-transition
           #:settings-volume-after-target
           #:stop-menu-sound
           #:wifi-activate-target
           #:wifi-apply-target
           #:wifi-complete-save
           #:wifi-controller-transition
           #:wifi-initial-state
           #:wifi-open-state
           #:wifi-save-plan
           #:wifi-tail-for-field
           #:wifi-target-at
           #:wifi-touch-transition
           #:wifi-valid-text-p
           #:stroke-canvas-rect
           #:wayland-shutdown-requested-p))

(in-package #:retrodeck)

(defconstant +native-abi-version+ 12)

(defparameter *menu-sound-cues*
  '((:volume (660 60) (880 60))
    (:previous (523 35))
    (:next (659 35))
    (:confirm (659 25) (880 30))
    (:back (659 25) (440 30))))

(defparameter *menu-sound-input-tail-ms* 60)
(defparameter *menu-sound-input-until-ms* 0)

(defun monotonic-ms ()
  (floor (* 1000 (get-internal-real-time))
         internal-time-units-per-second))

(defun menu-sound-notes (cue)
  (copy-tree (cdr (or (assoc cue *menu-sound-cues*)
                      (assoc :back *menu-sound-cues*)))))

(defun menu-sound-duration-ms (cue)
  (reduce #'+ (menu-sound-notes cue) :key #'second))

(defun play-menu-sound (cue volume-percent)
  (check-type volume-percent (integer 0 100))
  (when (zerop volume-percent)
    (return-from play-menu-sound (values t nil)))
  (let ((notes (menu-sound-notes cue)))
    (unless (<= 1 (length notes) 2)
      (error "Menu cues need one or two notes"))
    (destructuring-bind ((first-frequency first-duration)
                         &optional (second '(0 0)))
        notes
      (destructuring-bind (second-frequency second-duration) second
        (let* ((started-at (monotonic-ms))
               (status (play-tones first-frequency first-duration
                                   second-frequency second-duration
                                   volume-percent)))
          (when (= status 1)
            (setf *menu-sound-input-until-ms*
                  (+ started-at (menu-sound-duration-ms cue)
                     *menu-sound-input-tail-ms*)))
          (values (plusp status) (= status 1)))))))

(defun menu-sound-blocks-input-p (input-kind &optional (now (monotonic-ms)))
  (and (eq input-kind :controller)
       (or (= (audio-active-p) 1)
           (< now *menu-sound-input-until-ms*))))

(defun stop-menu-sound ()
  (stop-audio)
  (setf *menu-sound-input-until-ms* 0)
  t)

(defun finish-menu-sound ()
  (finish-audio)
  (setf *menu-sound-input-until-ms* 0)
  t)

(defun clear-canvas (color)
  (check-type color (integer 0 16777215))
  (= (canvas-clear color) 1))

(defun canvas-rgb565-hash ()
  (let ((words (canvas-rgb565-hash-words)))
    (unless (= (length words) 4)
      (error "Native canvas hash is unavailable"))
    (reduce (lambda (value word) (logior (ash value 16) word))
            words :initial-value 0)))

(defun native-unsigned-64-hex (value)
  (check-type value (integer 0 9223372036854775807))
  (coerce (format nil "~16,'0X" value) 'base-string))

(defun draw-canvas-glyph (x y character-code scale color)
  (check-type x (and fixnum (signed-byte 32)))
  (check-type y (and fixnum (signed-byte 32)))
  (check-type character-code (integer 0 255))
  (check-type scale (and fixnum (integer 1 4294967295)))
  (check-type color (integer 0 16777215))
  (= (canvas-draw-glyph x y character-code scale color) 1))

(defun fill-canvas-rect (x y width height color)
  (check-type x (and fixnum (signed-byte 32)))
  (check-type y (and fixnum (signed-byte 32)))
  (check-type width (and fixnum (unsigned-byte 32)))
  (check-type height (and fixnum (unsigned-byte 32)))
  (check-type color (integer 0 16777215))
  (= (canvas-fill-rect x y width height color) 1))

(defun native-path-string (path)
  (coerce (namestring (pathname path)) 'base-string))

(defun read-bounded-regular-file (path minimum-bytes maximum-bytes)
  (check-type minimum-bytes (and fixnum (integer 0 4194304)))
  (check-type maximum-bytes (and fixnum (integer 0 4194304)))
  (when (> minimum-bytes maximum-bytes)
    (error "Regular file byte bounds are invalid"))
  (read-regular-file (native-path-string path) minimum-bytes maximum-bytes))

(defun load-text-mask (text scale)
  (check-type text string)
  (check-type scale (and fixnum (integer 1 4294967295)))
  (let ((handle (text-mask-load
                 (coerce (display-ascii text) 'base-string) scale)))
    (and (plusp handle) handle)))

(defun clear-text-mask-cache ()
  (= (text-mask-clear) 1))

(defun configure-text-projection
    (elapsed-ms speed-numerator speed-denominator cycle camera-distance
     maximum-depth horizon-y clip-top fade-invisible-y fade-opaque-y bottom-y
     color)
  (check-type elapsed-ms (integer 0 9223372036854775807))
  (dolist (value (list speed-numerator speed-denominator cycle camera-distance
                       maximum-depth))
    (check-type value (and fixnum (unsigned-byte 32))))
  (dolist (value (list horizon-y clip-top fade-invisible-y fade-opaque-y
                       bottom-y))
    (check-type value (and fixnum (signed-byte 32))))
  (check-type color (integer 0 16777215))
  (= (canvas-configure-projection
      (native-unsigned-64-hex elapsed-ms)
      speed-numerator speed-denominator cycle camera-distance
      maximum-depth horizon-y clip-top fade-invisible-y fade-opaque-y bottom-y
      color)
     1))

(defun draw-projected-text (handle source-y)
  (check-type handle (and fixnum (integer 1 4294967295)))
  (check-type source-y (and fixnum (signed-byte 32)))
  (= (canvas-draw-projected-text handle source-y) 1))

(defun load-cover-raster (path background)
  (check-type background (integer 0 16777215))
  (let ((handle (retrodeck.native:raster-load-cover
                 (native-path-string path) background)))
    (and (plusp handle) handle)))

(defun load-png-raster (path width height)
  (check-type width (and fixnum (integer 1 2048)))
  (check-type height (and fixnum (integer 1 2048)))
  (let ((handle (retrodeck.native:raster-load-png
                 (native-path-string path) width height)))
    (and (plusp handle) handle)))

(defun draw-canvas-raster (handle x y width height)
  (check-type handle (and fixnum (integer 1 4294967295)))
  (check-type x (and fixnum (signed-byte 32)))
  (check-type y (and fixnum (signed-byte 32)))
  (check-type width (and fixnum (integer 1 4294967295)))
  (check-type height (and fixnum (integer 1 4294967295)))
  (= (canvas-draw-raster handle x y width height) 1))

(defun normalize-touch-report (report)
  (when report
    (destructuring-bind (x y down pressed released) report
      (list x y (plusp down) (plusp pressed) (plusp released)))))

(defun scan-evdev-controls ()
  (let ((counts (evdev-controls-scan)))
    (when counts
      (unless (and (listp counts)
                   (= (length counts) 2)
                   (typep (first counts) '(integer 0 2))
                   (typep (second counts) '(integer 0 4)))
        (error "Invalid native evdev control counts ~S" counts))
      (list :gamepads (first counts) :keyboards (second counts)))))

(defun close-evdev-controls ()
  (evdev-controls-close)
  t)

(defun dispatch-evdev-controls (&optional (timeout-ms 0))
  (check-type timeout-ms (integer 0 4294967295))
  (let ((result (evdev-controls-dispatch timeout-ms)))
    (when result
      (unless (and (listp result)
                   (= (length result) 2)
                   (typep (first result) '(integer 0 64))
                   (member (second result) '(0 1)))
        (error "Invalid native evdev dispatch result ~S" result))
      (list :count (first result) :rescan (plusp (second result))))))

(defun next-evdev-control ()
  (let ((report (evdev-next-control)))
    (when report
      (unless (and (listp report)
                   (= (length report) 3)
                   (every #'integerp report))
        (error "Invalid native evdev control report ~S" report))
      (destructuring-bind (kind value flags) report
        (case kind
          (0
           (unless (and (typep value '(integer 0 65535))
                        (typep flags '(integer 0 3)))
             (error "Invalid native keyboard report ~S" report))
           (list :kind :keyboard :code value
                 :shift (logbitp 0 flags)
                 :repeat (logbitp 1 flags)))
          (1
           (unless (and (typep value '(integer 1 4095)) (zerop flags))
             (error "Invalid native gamepad report ~S" report))
           (list :kind :gamepad :edges value))
          (otherwise
           (error "Unknown native evdev control report ~S" report)))))))

(defun open-evdev-touch ()
  (= (evdev-touch-open) 1))

(defun close-evdev-touch ()
  (evdev-touch-close)
  t)

(defun dispatch-evdev-touch (&optional (timeout-ms 0))
  (check-type timeout-ms (integer 0 *))
  (let ((dispatched (evdev-touch-dispatch timeout-ms)))
    (unless (minusp dispatched)
      dispatched)))

(defun next-evdev-touch ()
  (normalize-touch-report (evdev-next-touch)))

(defun open-fbdev ()
  (= (fbdev-open) 1))

(defun close-fbdev ()
  (fbdev-close)
  t)

(defun present-fbdev-canvas ()
  (= (fbdev-present-canvas) 1))

(defun present-fbdev-solid (color)
  (check-type color (integer 0 16777215))
  (= (fbdev-present-solid color) 1))

(defun current-fbdev-size ()
  (fbdev-size))

(defun open-wayland-widget ()
  (= (wayland-open-widget) 1))

(defun close-wayland ()
  (wayland-close)
  t)

(defun present-wayland-canvas ()
  (= (wayland-present-canvas) 1))

(defun present-wayland-solid (color)
  (check-type color (integer 0 16777215))
  (= (wayland-present-solid color) 1))

(defun dispatch-wayland (&optional (timeout-ms 0))
  (check-type timeout-ms (integer 0 *))
  (let ((dispatched (wayland-dispatch timeout-ms)))
    (unless (minusp dispatched)
      dispatched)))

(defun next-wayland-touch ()
  (normalize-touch-report (wayland-next-touch)))

(defun current-wayland-size ()
  (wayland-size))

(defun wayland-shutdown-requested-p ()
  (= (wayland-shutdown-p) 1))

(defun main ()
  (unless (= (abi-version) +native-abi-version+)
    (error "Native ABI mismatch"))
  (format t "retrodeck: Common Lisp startup loaded~%")
  (finish-output)
  0)

(let ((startup *load-truename*))
  (load (merge-pathnames "ui.lisp" startup) :verbose nil :print nil)
  (load (merge-pathnames "policy.lisp" startup) :verbose nil :print nil)
  (load (merge-pathnames "process.lisp" startup) :verbose nil :print nil)
  (load (merge-pathnames "settings.lisp" startup) :verbose nil :print nil)
  (load (merge-pathnames "wifi.lisp" startup) :verbose nil :print nil)
  (load (merge-pathnames "credits.lisp" startup) :verbose nil :print nil)
  (load (merge-pathnames "dashboard.lisp" startup) :verbose nil :print nil)
  (let ((local (merge-pathnames "local.lisp" startup)))
    (when (probe-file local)
      (load local :verbose nil :print nil))))

(defpackage #:retrodeck.native
  (:use)
  (:export #:abi-version
           #:audio-active-p
           #:canvas-clear
           #:canvas-draw-glyph
           #:canvas-draw-raster
           #:canvas-fill-rect
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
           #:stop-audio
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
                #:canvas-draw-glyph
                #:canvas-draw-raster
                #:canvas-fill-rect
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
                #:stop-audio
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
           #:*dashboard-cover-directory*
           #:*dashboard-executables*
           #:*dashboard-menu-geometry*
           #:*dashboard-palette*
           #:*dashboard-reboot-confirmation-text*
           #:*dashboard-reduced-motion-environment*
           #:*dashboard-settings-icon-path*
           #:*dashboard-systems*
           #:*dashboard-terminal-login-shell*
           #:*dashboard-timings*
           #:*dashboard-volume-default*
           #:*dashboard-volume-step*
           #:*menu-sound-cues*
           #:*menu-sound-input-tail-ms*
           #:bitmap-text-width
           #:clear-canvas
           #:clear-dashboard-raster-cache
           #:close-evdev-touch
           #:close-fbdev
           #:close-wayland
           #:current-fbdev-size
           #:current-wayland-size
           #:dashboard-application
           #:dashboard-color
           #:dashboard-executable
           #:dashboard-initial-state
           #:dashboard-launch-plan
           #:dashboard-menu-geometry
           #:dashboard-system-label
           #:dashboard-target-at
           #:dashboard-timing
           #:dashboard-touch-transition
           #:dispatch-evdev-touch
           #:dispatch-wayland
           #:display-ascii
           #:draw-canvas-glyph
           #:draw-canvas-raster
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
           #:main
           #:menu-sound-blocks-input-p
           #:menu-sound-duration-ms
           #:menu-sound-notes
           #:next-evdev-touch
           #:next-wayland-touch
           #:open-evdev-touch
           #:open-fbdev
           #:open-wayland-widget
           #:play-menu-sound
           #:prepare-dashboard-rasters
           #:present-fbdev-canvas
           #:present-fbdev-solid
           #:present-wayland-canvas
           #:present-wayland-solid
           #:reboot-confirmation-active-p
           #:render-dashboard
           #:stop-menu-sound
           #:stroke-canvas-rect
           #:wayland-shutdown-requested-p))

(in-package #:retrodeck)

(defconstant +native-abi-version+ 8)

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
    (return-from play-menu-sound t))
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
          (plusp status))))))

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
  (load (merge-pathnames "dashboard.lisp" startup) :verbose nil :print nil)
  (let ((local (merge-pathnames "local.lisp" startup)))
    (when (probe-file local)
      (load local :verbose nil :print nil))))

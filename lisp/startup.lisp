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

(defpackage #:retrodeck
  (:use #:cl)
  (:import-from #:retrodeck.native
                #:abi-version
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
                #:wayland-size)
  (:export #:*dashboard-brightness-minimum*
           #:*dashboard-brightness-step*
           #:*dashboard-built-in-applications*
           #:*dashboard-controller-burst-limit*
           #:*dashboard-executables*
           #:*dashboard-palette*
           #:*dashboard-reboot-confirmation-text*
           #:*dashboard-reduced-motion-environment*
           #:*dashboard-systems*
           #:*dashboard-terminal-login-shell*
           #:*dashboard-timings*
           #:*dashboard-volume-default*
           #:*dashboard-volume-step*
           #:*menu-sound-cues*
           #:*menu-sound-input-tail-ms*
           #:close-wayland
           #:current-wayland-size
           #:dashboard-application
           #:dashboard-color
           #:dashboard-executable
           #:dashboard-launch-plan
           #:dashboard-system-label
           #:dashboard-timing
           #:dispatch-wayland
           #:finish-menu-sound
           #:main
           #:menu-sound-blocks-input-p
           #:menu-sound-duration-ms
           #:menu-sound-notes
           #:next-wayland-touch
           #:open-wayland-widget
           #:play-menu-sound
           #:present-wayland-solid
           #:reboot-confirmation-active-p
           #:stop-menu-sound
           #:wayland-shutdown-requested-p))

(in-package #:retrodeck)

(defconstant +native-abi-version+ 3)

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

(defun open-wayland-widget ()
  (= (wayland-open-widget) 1))

(defun close-wayland ()
  (wayland-close)
  t)

(defun present-wayland-solid (color)
  (check-type color (integer 0 16777215))
  (= (wayland-present-solid color) 1))

(defun dispatch-wayland (&optional (timeout-ms 0))
  (check-type timeout-ms (integer 0 *))
  (let ((dispatched (wayland-dispatch timeout-ms)))
    (unless (minusp dispatched)
      dispatched)))

(defun next-wayland-touch ()
  (let ((report (wayland-next-touch)))
    (when report
      (destructuring-bind (x y down pressed released) report
        (list x y (plusp down) (plusp pressed) (plusp released))))))

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
  (load (merge-pathnames "policy.lisp" startup) :verbose nil :print nil)
  (let ((local (merge-pathnames "local.lisp" startup)))
    (when (probe-file local)
      (load local :verbose nil :print nil))))

(in-package #:retrodeck)

(defparameter *dashboard-wifi-labels*
  '(:back "BACK"
    :title "ADD WIFI"
    :ssid "SSID"
    :password "PASSWORD"
    :save "SAVE NETWORK"
    :alphabet-mode "ABC"
    :symbol-mode "123"
    :lowercase "abc"
    :uppercase "ABC"
    :space "SPACE"
    :delete "DELETE"
    :default-status "SAVING DOES NOT INTERRUPT CURRENT WIFI"
    :network-wifi "WIFI"
    :network-wlan "WLAN0"
    :network-wireguard "WG0"
    :not-connected "NOT CONNECTED"
    :no-address "NO ADDRESS"
    :auto-wifi-prefix "AUTO WIFI: "
    :ssid-error "SSID MUST BE 1 TO 32 CHARACTERS"
    :passphrase-error "PASSWORD MUST BE 8 TO 63 CHARACTERS"
    :save-success "WIFI SAVED - USED AFTER CURRENT WIFI DISCONNECTS"
    :save-failure "WIFI PROFILE WAS NOT SAVED"
    :closed "WIFI EDITOR CLOSED"))

(defparameter *dashboard-wifi-geometry*
  '(:canvas-width 1280
    :back (16 10 120 62)
    :title (158 25 3)
    :ssid (330 10 310 62)
    :passphrase (650 10 330 62)
    :save (990 10 274 62)
    :field-label-offset (10 7 1)
    :field-value-offset (10 28 2)
    :ssid-tail 19
    :passphrase-tail 20
    :key-row-y (86 154 222 290)
    :key-margin 16
    :key-gap 6
    :key-height 62
    :mode (16 364 152 66)
    :shift (176 364 168 66)
    :space (352 364 700 66)
    :delete (1060 364 204 66)
    :footer (12 436 1256 10 1)
    :network (12 450 1256 10 1 1248)
    :selector (12 464 1256 10 1 1248)))

(defparameter *dashboard-wifi-key-rows*
  '(:alphabet ("qwertyuiop" "asdfghjkl" "zxcvbnm" "@._-")
    :symbols ("1234567890" "!@#$%^&*()" "-_=+[]{}\\|"
              "`~;:'\",./?<>") ))

(defparameter *dashboard-wifi-limits*
  '(:ssid-minimum 1 :ssid-maximum 32
    :passphrase-minimum 8 :passphrase-maximum 63))

(defparameter *dashboard-wifi-paths*
  '(:profile-helper "/usr/sbin/deck-wifi-profile-add"))

(defun dashboard-wifi-value (values name description)
  (let ((missing (gensym)))
    (let ((value (getf values name missing)))
      (if (eq value missing)
          (error "Unknown dashboard Wi-Fi ~A ~S" description name)
          (if (listp value) (copy-tree value) value)))))

(defun dashboard-wifi-label (name)
  (dashboard-wifi-value *dashboard-wifi-labels* name "label"))

(defun dashboard-wifi-geometry (name)
  (dashboard-wifi-value *dashboard-wifi-geometry* name "geometry"))

(defun dashboard-wifi-key-rows (mode)
  (dashboard-wifi-value *dashboard-wifi-key-rows* mode "key rows"))

(defun dashboard-wifi-limit (name)
  (dashboard-wifi-value *dashboard-wifi-limits* name "limit"))

(defun dashboard-wifi-path (name)
  (dashboard-wifi-value *dashboard-wifi-paths* name "path"))

(defun wifi-tail-for-field (value maximum)
  (check-type value string)
  (check-type maximum (integer 0 *))
  (let ((length (length value)))
    (cond
      ((<= length maximum) value)
      ((<= maximum 3) (subseq value (- length maximum)))
      (t (concatenate 'string "..."
                      (subseq value (- length (- maximum 3))))))))

(defun draw-wifi-button (bounds label active)
  (destructuring-bind (x y width height) bounds
    (fill-canvas-rect x y width height
                      (dashboard-color (if active :wifi-active :surface)))
    (stroke-canvas-rect x y width height 3
                        (dashboard-color
                         (if active :wifi-active-border :control-border)))
    (draw-centered-text x y width height label
                        (fit-text-scale label (- width 12) 3 1)
                        (dashboard-color :white))))

(defun wifi-uppercase-character (character uppercase)
  (if (and uppercase (char<= #\a character #\z))
      (code-char (+ (char-code #\A) (- (char-code character)
                                       (char-code #\a))))
      character))

(defun draw-wifi-key-row (values y uppercase)
  (let* ((canvas-width (dashboard-wifi-geometry :canvas-width))
         (gap (dashboard-wifi-geometry :key-gap))
         (margin (dashboard-wifi-geometry :key-margin))
         (height (dashboard-wifi-geometry :key-height))
         (count (length values))
         (width (floor (- canvas-width (* 2 margin) (* gap (1- count)))
                       count))
         (used (+ (* count width) (* gap (1- count))))
         (left (floor (- canvas-width used) 2))
         (keys nil))
    (loop for index from 0 below count
          for source = (char values index)
          for value = (wifi-uppercase-character source uppercase)
          for bounds = (list (+ left (* index (+ width gap)))
                             y width height)
          do (push (list bounds value) keys)
             (draw-wifi-button bounds (string value) nil))
    (nreverse keys)))

(defun wifi-network-text (network key fallback)
  (let ((value (getf network key)))
    (if (and (stringp value) (plusp (length value))) value fallback)))

(defun render-dashboard-wifi (state network)
  (check-type state list)
  (check-type network list)
  (let* ((back (dashboard-wifi-geometry :back))
         (ssid (dashboard-wifi-geometry :ssid))
         (passphrase (dashboard-wifi-geometry :passphrase))
         (save (dashboard-wifi-geometry :save))
         (mode (dashboard-wifi-geometry :mode))
         (shift (dashboard-wifi-geometry :shift))
         (space (dashboard-wifi-geometry :space))
         (delete (dashboard-wifi-geometry :delete))
         (symbols (getf state :symbols))
         (uppercase (getf state :uppercase))
         (field (getf state :field))
         (keys nil))
    (clear-canvas (dashboard-color :background))
    (draw-wifi-button back (dashboard-wifi-label :back) nil)
    (destructuring-bind (x y scale) (dashboard-wifi-geometry :title)
      (draw-text x y (dashboard-wifi-label :title) scale
                 (dashboard-color :title)))
    (flet ((draw-field (bounds label value selected)
             (destructuring-bind (x y width height) bounds
               (fill-canvas-rect x y width height (dashboard-color :field))
               (stroke-canvas-rect
                x y width height 3
                (dashboard-color
                 (if selected :wifi-focus :inactive-border)))
               (destructuring-bind (offset-x offset-y scale)
                   (dashboard-wifi-geometry :field-label-offset)
                 (draw-text (+ x offset-x) (+ y offset-y) label scale
                            (dashboard-color :field-label)))
               (destructuring-bind (offset-x offset-y scale)
                   (dashboard-wifi-geometry :field-value-offset)
                 (draw-text (+ x offset-x) (+ y offset-y) value scale
                            (dashboard-color :white))))))
      (draw-field ssid (dashboard-wifi-label :ssid)
                  (wifi-tail-for-field
                   (getf state :ssid) (dashboard-wifi-geometry :ssid-tail))
                  (eq field :ssid))
      (draw-field passphrase (dashboard-wifi-label :password)
                  (wifi-tail-for-field
                   (make-string (length (getf state :passphrase))
                                :initial-element #\*)
                   (dashboard-wifi-geometry :passphrase-tail))
                  (eq field :passphrase)))
    (draw-wifi-button save (dashboard-wifi-label :save) nil)
    (loop for values in (dashboard-wifi-key-rows
                         (if symbols :symbols :alphabet))
          for y in (dashboard-wifi-geometry :key-row-y)
          do (setf keys
                   (nconc keys
                          (draw-wifi-key-row values y
                                             (and uppercase (not symbols))))))
    (draw-wifi-button mode
                      (dashboard-wifi-label
                       (if symbols :alphabet-mode :symbol-mode))
                      symbols)
    (draw-wifi-button shift
                      (dashboard-wifi-label
                       (if uppercase :uppercase :lowercase))
                      (and (not symbols) uppercase))
    (draw-wifi-button space (dashboard-wifi-label :space) nil)
    (draw-wifi-button delete (dashboard-wifi-label :delete) nil)
    (destructuring-bind (x y width height scale)
        (dashboard-wifi-geometry :footer)
      (draw-centered-text
       x y width height
       (if (plusp (length (getf state :status)))
           (getf state :status)
           (dashboard-wifi-label :default-status))
       scale (dashboard-color :footer)))
    (let ((addresses
            (format nil "~A ~A  ~A ~A  ~A ~A"
                    (dashboard-wifi-label :network-wifi)
                    (wifi-network-text
                     network :ssid (dashboard-wifi-label :not-connected))
                    (dashboard-wifi-label :network-wlan)
                    (wifi-network-text
                     network :wlan-ipv4 (dashboard-wifi-label :no-address))
                    (dashboard-wifi-label :network-wireguard)
                    (wifi-network-text
                     network :wireguard-ipv4
                     (dashboard-wifi-label :no-address)))))
      (destructuring-bind (x y width height scale maximum-width)
          (dashboard-wifi-geometry :network)
        (draw-centered-text x y width height
                            (fit-text-width addresses maximum-width scale)
                            scale (dashboard-color :text))))
    (let ((selector
            (concatenate 'string
                         (dashboard-wifi-label :auto-wifi-prefix)
                         (or (getf network :selector) ""))))
      (destructuring-bind (x y width height scale maximum-width)
          (dashboard-wifi-geometry :selector)
        (draw-centered-text x y width height
                            (fit-text-width selector maximum-width scale)
                            scale (dashboard-color :muted))))
    (list :back back :ssid ssid :passphrase passphrase :save save
          :mode mode :shift shift :space space :delete delete :keys keys)))

(defun wifi-bounds-contains-p (bounds x y)
  (destructuring-bind (left top width height) bounds
    (and (<= left x) (< x (+ left width))
         (<= top y) (< y (+ top height)))))

(defun wifi-target-at (layout x y)
  (check-type layout list)
  (dolist (target '(:back :ssid :passphrase :save
                    :mode :shift :space :delete))
    (when (wifi-bounds-contains-p (getf layout target) x y)
      (return-from wifi-target-at target)))
  (loop for key in (getf layout :keys)
        for index from 0
        when (wifi-bounds-contains-p (first key) x y)
          return (list :key index (second key))))

(defun wifi-initial-state (&key (ssid "") (passphrase "") (field :ssid)
                                uppercase symbols (status ""))
  (check-type ssid string)
  (check-type passphrase string)
  (unless (member field '(:ssid :passphrase))
    (error "Unknown Wi-Fi field ~S" field))
  (check-type status string)
  (list :open t :ssid ssid :passphrase passphrase :field field
        :uppercase (not (null uppercase)) :symbols (not (null symbols))
        :status status :pressed-target nil))

(defun wifi-open-state (state)
  (let ((next (copy-list state)))
    (setf (getf next :open) t
          (getf next :status) ""
          (getf next :pressed-target) nil)
    next))

(defun wifi-current-field-limit (state)
  (dashboard-wifi-limit
   (if (eq (getf state :field) :ssid) :ssid-maximum :passphrase-maximum)))

(defun wifi-current-field-value (state)
  (getf state (if (eq (getf state :field) :ssid) :ssid :passphrase)))

(defun wifi-set-current-field-value (state value)
  (setf (getf state (if (eq (getf state :field) :ssid)
                        :ssid :passphrase))
        value)
  state)

(defun wifi-key-target-index (target)
  (and (consp target) (eq (first target) :key)
       (integerp (second target)) (<= 0 (second target))
       (second target)))

(defun wifi-apply-target (state layout target)
  (let ((next (copy-list state))
        (applied t))
    (case target
      (:ssid (setf (getf next :field) :ssid))
      (:passphrase (setf (getf next :field) :passphrase))
      (:mode (setf (getf next :symbols) (not (getf next :symbols))))
      (:shift
       (if (getf next :symbols)
           (setf applied nil)
           (setf (getf next :uppercase) (not (getf next :uppercase)))))
      (otherwise
       (let* ((value (wifi-current-field-value next))
              (limit (wifi-current-field-limit next))
              (key-index (wifi-key-target-index target)))
         (cond
           ((eq target :delete)
            (when (plusp (length value))
              (wifi-set-current-field-value
               next (subseq value 0 (1- (length value))))))
           ((eq target :space)
            (when (< (length value) limit)
              (wifi-set-current-field-value
               next (concatenate 'string value " "))))
           ((and key-index
                 (< key-index (length (getf layout :keys))))
            (when (< (length value) limit)
              (wifi-set-current-field-value
               next (concatenate
                     'string value
                     (string (second (nth key-index (getf layout :keys))))))))
           (t (setf applied nil))))))
    (when applied
      (setf (getf next :status) ""))
    (values next applied)))

(defun wifi-valid-text-p (value minimum maximum)
  (and (<= minimum (length value) maximum)
       (every (lambda (character)
                (<= #x20 (char-code character) #x7e))
              value)))

(defun wifi-save-plan (state)
  (let ((ssid (getf state :ssid))
        (passphrase (getf state :passphrase)))
    (cond
      ((not (wifi-valid-text-p
             ssid (dashboard-wifi-limit :ssid-minimum)
             (dashboard-wifi-limit :ssid-maximum)))
       (values nil (dashboard-wifi-label :ssid-error)))
      ((not (wifi-valid-text-p
             passphrase (dashboard-wifi-limit :passphrase-minimum)
             (dashboard-wifi-limit :passphrase-maximum)))
       (values nil (dashboard-wifi-label :passphrase-error)))
      (t
       (values
        (list :action :save
              :executable (dashboard-wifi-path :profile-helper)
              :input (format nil "~A~%~A~%" ssid passphrase)
              :success-status (dashboard-wifi-label :save-success)
              :failure-status (dashboard-wifi-label :save-failure)
              :cue :confirm)
        nil)))))

(defun wifi-complete-save (state plan succeeded-p &key failure-status)
  (unless (eq (getf plan :action) :save)
    (error "Unknown Wi-Fi completion plan ~S" plan))
  (let ((next (copy-list state)))
    (if succeeded-p
        (setf (getf next :passphrase) ""
              (getf next :status) (getf plan :success-status))
        (setf (getf next :status)
              (or failure-status (getf plan :failure-status))))
    (values next (list :cue (getf plan :cue)))))

(defun wifi-activate-target (state layout target)
  (case target
    (:back
     (let ((next (copy-list state)))
       (setf (getf next :open) nil)
       (values next
               (list :action :close
                     :dashboard-status (dashboard-wifi-label :closed)
                     :cue :back))))
    (:save
     (multiple-value-bind (plan error-status) (wifi-save-plan state)
       (if plan
           (values (copy-list state) plan)
           (let ((next (copy-list state)))
             (setf (getf next :status) error-status)
             (values next '(:cue :confirm))))))
    (otherwise
     (multiple-value-bind (next applied)
         (wifi-apply-target state layout target)
       (values next (and applied '(:cue :next)))))))

(defun wifi-controller-transition (state command)
  (if (eq command :back)
      (wifi-activate-target state nil :back)
      (values (copy-list state) nil)))

(defun wifi-touch-transition (state layout report)
  (destructuring-bind (x y down pressed released) report
    (declare (ignore down))
    (let* ((next (copy-list state))
           (target (wifi-target-at layout x y)))
      (when pressed
        (setf (getf next :pressed-target) target))
      (if released
          (let ((pressed-target (getf next :pressed-target)))
            (setf (getf next :pressed-target) nil)
            (if (and target (equal pressed-target target))
                (wifi-activate-target next layout target)
                (values next nil)))
          (values next nil)))))

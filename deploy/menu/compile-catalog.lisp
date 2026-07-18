;;;; Validate games.sexp and atomically emit the Deck menu's TSV manifest.
;;;; The catalog data is deliberately limited to printable ASCII so the same
;;;; output is produced regardless of the minimal ECL runtime's locale.

(defpackage #:nes-deck-catalog
  (:use #:common-lisp))

(in-package #:nes-deck-catalog)

(defconstant +schema-version+ 6)
(defconstant +appearance-override-version+ 3)
(defconstant +maximum-games+ 64)
(defconstant +maximum-catalog-bytes+ 65536)
(defconstant +console-rom-root+ "/mnt/data/roms/")
(defconstant +deck-game-root+ "/mnt/data/nes-deck/games/")
(defparameter +catalog-keys+ '(:version :settings-icon :palette :games))
(defparameter +game-keys+
  '(:id :title :system :rom :color))
(defparameter +palette-keys+
  '(:background :text-dark :field :surface :inactive-border
    :control-border :footer :inactive-text :text :white :title
    :volume-off :volume-on :selected :wifi-active :wifi-focus
    :wifi-active-border :field-label :accent :active :control-surface
    :muted))
(defparameter +appearance-override-keys+
  '(:version :settings-icon :palette))
(defparameter +settings-icons+
  (append
   '("gear-classic" "gear-square" "gear-diamond" "gear-eight"
     "gear-spoke" "gear-ring" "gear-cross" "gear-compact"
     "gear-heavy" "gear-rivet" "gear-outline" "gear-steel-outline")
   (loop for index from 1 to 36
         collect (format nil "gear-knekko-~2,'0D" index))))

(defun catalog-error (control &rest arguments)
  (error "Catalog error: ~?" control arguments))

(defun bounded-list-elements (value limit context &key allow-empty)
  "Return VALUE as a fresh list, rejecting dotted, circular, and long lists."
  (let ((cursor value)
        (result '())
        (count 0))
    (loop
      (cond
        ((null cursor)
         (when (and (null result) (not allow-empty))
           (catalog-error "~A must not be empty" context))
         (return (nreverse result)))
        ((not (consp cursor))
         (catalog-error "~A must be a proper list" context))
        ((>= count limit)
         (catalog-error "~A exceeds the limit of ~D elements or is circular"
                        context limit))
        (t
         (push (car cursor) result)
         (setf cursor (cdr cursor))
         (incf count))))))

(defun decode-plist (value allowed-keys context
                     &optional (required-keys allowed-keys))
  "Decode a short property list and reject missing, duplicate, or unknown keys."
  (let* ((elements
           ;; Permit one extra pair through the bounded traversal so the
           ;; validator can report an unknown or duplicate key precisely.
           (bounded-list-elements
            value (+ 2 (* 2 (length allowed-keys))) context))
         (element-count (length elements))
         (pairs '()))
    (unless (evenp element-count)
      (catalog-error "~A has a key without a value" context))
    (loop for (key field-value) on elements by #'cddr do
      (unless (member key allowed-keys :test #'eq)
        (catalog-error "~A contains unknown key ~S" context key))
      (when (assoc key pairs :test #'eq)
        (catalog-error "~A contains duplicate key ~S" context key))
      (push (cons key field-value) pairs))
    (dolist (key required-keys)
      (unless (assoc key pairs :test #'eq)
        (catalog-error "~A is missing key ~S" context key)))
    (nreverse pairs)))

(defun required-value (key pairs)
  (cdr (assoc key pairs :test #'eq)))

(defun printable-ascii-string-p (value)
  (and (stringp value)
       (every (lambda (character)
                (let ((code (char-code character)))
                  (and (<= 32 code) (<= code 126))))
              value)))

(defun validate-text-field (value name maximum-length)
  (unless (and (printable-ascii-string-p value)
               (plusp (length value))
               (<= (length value) maximum-length))
    (catalog-error
     "~A must be 1 to ~D printable ASCII characters with no tabs or newlines"
     name maximum-length))
  value)

(defun validate-trimmed-text-field (value name maximum-length)
  (validate-text-field value name maximum-length)
  (unless (string= value (string-trim '(#\Space) value))
    (catalog-error "~A must not have leading or trailing spaces" name))
  value)

(defun lowercase-id-character-p (character)
  (or (and (char<= #\a character) (char<= character #\z))
      (digit-char-p character)))

(defun validate-id (value)
  (validate-text-field value "game :id" 32)
  (unless (and (lowercase-id-character-p (char value 0))
               (every (lambda (character)
                        (or (lowercase-id-character-p character)
                            (char= character #\-)))
                      value)
               (not (char= (char value (1- (length value))) #\-))
               (not (search "--" value)))
    (catalog-error
     "game :id must use lowercase letters, digits, and single interior hyphens"))
  value)

(defun string-prefix-p (prefix value)
  (and (<= (length prefix) (length value))
       (string= prefix value :end2 (length prefix))))

(defun string-suffix-p (suffix value)
  (let ((start (- (length value) (length suffix))))
    (and (not (minusp start))
         (string= suffix value :start2 start))))

(defun rom-path-character-p (character)
  (or (alphanumericp character)
      (find character "/._-" :test #'char=)))

(defun validate-system (value)
  (unless (and (symbolp value)
               (member value '(:nes :gb :gbc :zx :chip8 :deck) :test #'eq))
    (catalog-error
     "game :system must be one of :nes, :gb, :gbc, :zx, :chip8, or :deck"))
  (string-downcase (symbol-name value)))

(defun validate-rom-path (value system)
  (validate-text-field value "game :rom" 512)
  (let* ((system-name (string-downcase (symbol-name system)))
         (expected-prefix
           (if (eq system :deck)
               +deck-game-root+
               (format nil "~A~A/" +console-rom-root+ system-name)))
         (expected-suffix
          (cond ((eq system :nes) ".nes")
                ((eq system :gb) ".gb")
                ((eq system :gbc) ".gbc")
                ((eq system :zx) ".tap")
                ((eq system :chip8) ".ch8")
                ((eq system :deck) nil)
                (t (catalog-error "unsupported game system ~S" system)))))
    (unless (and (string-prefix-p expected-prefix value)
               (or (null expected-suffix)
                   (string-suffix-p expected-suffix value))
               (every #'rom-path-character-p value)
               (not (search "//" value))
               (not (search "/./" value))
               (not (search "/../" value)))
      (if expected-suffix
          (catalog-error
           "game :rom must be a normalized ~A path below ~A"
           expected-suffix expected-prefix)
          (catalog-error
           "game :rom must be a normalized path below ~A"
           expected-prefix))))
  value)

(defun hexadecimal-character-p (character)
  (or (digit-char-p character)
      (find character "ABCDEFabcdef" :test #'char=)))

(defun xterm-cube-component-p (component)
  (member component '(0 95 135 175 215 255) :test #'eql))

(defun xterm-color-p (value)
  (let* ((red (parse-integer value :start 1 :end 3 :radix 16))
         (green (parse-integer value :start 3 :end 5 :radix 16))
         (blue (parse-integer value :start 5 :end 7 :radix 16))
         (rgb (+ (ash red 16) (ash green 8) blue)))
    (or (member rgb
                '(#x000000 #x800000 #x008000 #x808000
                  #x000080 #x800080 #x008080 #xC0C0C0
                  #x808080 #xFF0000 #x00FF00 #xFFFF00
                  #x0000FF #xFF00FF #x00FFFF #xFFFFFF)
                :test #'eql)
        (and (xterm-cube-component-p red)
             (xterm-cube-component-p green)
             (xterm-cube-component-p blue))
        (and (= red green blue)
             (<= 8 red 238)
             (zerop (mod (- red 8) 10))))))

(defun validate-color (value)
  (unless (and (stringp value)
               (= (length value) 7)
               (char= (char value 0) #\#)
               (every #'hexadecimal-character-p (subseq value 1)))
    (catalog-error "game :color must have the form #RRGGBB"))
  (unless (xterm-color-p value)
    (catalog-error "game :color must be from the xterm-256 palette"))
  (string-upcase value))

(defun validate-rgb-color (value key)
  (unless (and (stringp value)
               (= (length value) 7)
               (char= (char value 0) #\#)
               (every #'hexadecimal-character-p (subseq value 1)))
    (catalog-error "palette ~S must have the form #RRGGBB" key))
  (string-upcase value))

(defun validate-palette (form context)
  (let ((pairs (decode-plist form +palette-keys+ context)))
    (loop for key in +palette-keys+
          collect
          (list (string-downcase (symbol-name key))
                (validate-rgb-color (required-value key pairs) key)))))

(defun validate-settings-icon (value context)
  (unless (and (stringp value)
               (member value +settings-icons+ :test #'string=))
    (catalog-error "~A must name a built-in pixel cog" context))
  value)

(defun validate-game (form position)
  (let* ((context (format nil "game ~D" position))
         (pairs (decode-plist form +game-keys+ context))
         (system (required-value :system pairs)))
    (list
     (validate-id (required-value :id pairs))
     (validate-trimmed-text-field
      (required-value :title pairs) "game :title" 48)
     (validate-system system)
     (validate-rom-path (required-value :rom pairs) system)
     (validate-color (required-value :color pairs)))))

(defun duplicate-field (games index)
  (loop for tail on games
        for value = (nth index (car tail))
        when (find value (cdr tail)
                   :key (lambda (game) (nth index game))
                   :test #'string=)
          do (return value)))

(defun validate-catalog (form)
  (let* ((pairs (decode-plist form +catalog-keys+ "catalog"))
         (version (required-value :version pairs))
         (settings-icon
           (validate-settings-icon (required-value :settings-icon pairs)
                                   "catalog :settings-icon"))
         (palette (validate-palette (required-value :palette pairs)
                                    "catalog :palette"))
         (raw-games
           (bounded-list-elements
            (required-value :games pairs) +maximum-games+ "catalog :games"))
         (games
           (loop for raw-game in raw-games
                 for position from 1
                 collect (validate-game raw-game position))))
    (unless (eql version +schema-version+)
      (catalog-error "unsupported :version ~S; expected ~D"
                     version +schema-version+))
    (dolist (field '((0 . ":id") (3 . ":rom")))
      (let ((duplicate (duplicate-field games (car field))))
        (when duplicate
          (catalog-error "duplicate game ~A ~S" (cdr field) duplicate))))
    (values games palette settings-icon)))

(defun validate-appearance-override (form)
  (let* ((pairs (decode-plist form +appearance-override-keys+
                              "appearance override"
                              '(:version :palette)))
         (version (required-value :version pairs)))
    (unless (member version '(2 3) :test #'eql)
      (catalog-error
       "unsupported appearance override :version ~S; expected 2 or ~D"
       version +appearance-override-version+))
    (let ((settings-pair (assoc :settings-icon pairs :test #'eq)))
      (when (and (eql version 2) settings-pair)
        (catalog-error "appearance override version 2 cannot set an icon"))
      (when (and (eql version +appearance-override-version+)
                 (null settings-pair))
        (catalog-error "appearance override version 3 is missing :settings-icon"))
      (values
       (validate-palette (required-value :palette pairs)
                         "appearance override :palette")
       (when settings-pair
         (validate-settings-icon (cdr settings-pair)
                                 "appearance override :settings-icon"))))))

(defun read-catalog (source)
  (with-open-file (stream source :direction :input)
    (let ((source-length (file-length stream)))
      (when (and source-length (> source-length +maximum-catalog-bytes+))
        (catalog-error "~A exceeds the ~D-byte input limit"
                       source +maximum-catalog-bytes+)))
    (let ((*read-eval* nil)
          (end-marker (cons nil nil)))
      (let ((form (read stream nil end-marker)))
        (when (eq form end-marker)
          (catalog-error "~A is empty" source))
        (unless (eq (read stream nil end-marker) end-marker)
          (catalog-error "~A contains more than one top-level form" source))
        form))))

(defun write-tsv (games stream)
  (dolist (game games)
    (loop for field in game
          for first = t then nil
          do (unless first (write-char #\Tab stream))
             (write-string field stream))
    (terpri stream)))

(defun write-palette-tsv (palette settings-icon stream)
  (write-string "settings-icon" stream)
  (write-char #\Tab stream)
  (write-string settings-icon stream)
  (terpri stream)
  (dolist (entry palette)
    (write-string (first entry) stream)
    (write-char #\Tab stream)
    (write-string (second entry) stream)
    (terpri stream)))

(defun process-id ()
  #+ecl (ext:getpid)
  #+sbcl (sb-unix:unix-getpid)
  #-(or ecl sbcl) (catalog-error "unsupported Common Lisp implementation"))

(defun replace-file (source destination)
  #+ecl (rename-file source destination :if-exists :supersede)
  #+sbcl (sb-unix:unix-rename source destination)
  #-(or ecl sbcl) (catalog-error "unsupported Common Lisp implementation"))

(defun emit-outputs-atomically
    (games palette settings-icon games-output palette-output)
  ;; Temporary files remain beside their outputs so both final renames are
  ;; atomic on the Deck's persistent filesystem.
  (let* ((games-temporary
           (format nil "~A.tmp.~D" games-output (process-id)))
         (palette-temporary
           (format nil "~A.tmp.~D" palette-output (process-id)))
         (games-committed nil)
         (palette-committed nil))
    (unwind-protect
         (progn
           (with-open-file (stream games-temporary
                                   :direction :output
                                   :if-exists :supersede
                                   :if-does-not-exist :create)
             (write-tsv games stream)
             (finish-output stream))
           (with-open-file (stream palette-temporary
                                   :direction :output
                                   :if-exists :supersede
                                   :if-does-not-exist :create)
             (write-palette-tsv palette settings-icon stream)
             (finish-output stream))
           (replace-file palette-temporary palette-output)
           (setf palette-committed t)
           (replace-file games-temporary games-output)
           (setf games-committed t))
      (unless games-committed
        (when (probe-file games-temporary)
          (ignore-errors (delete-file games-temporary))))
      (unless palette-committed
        (when (probe-file palette-temporary)
          (ignore-errors (delete-file palette-temporary)))))))

(defun command-arguments ()
  #+ecl (ext:command-args)
  #+sbcl sb-ext:*posix-argv*
  #-(or ecl sbcl) (catalog-error "unsupported Common Lisp implementation"))

(defun last-four-command-arguments ()
  ;; ECL includes its own options in EXT:COMMAND-ARGS.  The four fixed launcher
  ;; arguments are intentionally taken from the tail.
  (let* ((arguments (command-arguments))
         (count (length arguments)))
    (when (< count 4)
      (catalog-error
       "usage: ecl --norc --shell compile-catalog.lisp SOURCE GAMES-OUTPUT PALETTE-OUTPUT PALETTE-OVERRIDE"))
    (values (nth (- count 4) arguments)
            (nth (- count 3) arguments)
            (nth (- count 2) arguments)
            (nth (1- count) arguments))))

(defun main ()
  (multiple-value-bind
        (source games-output palette-output palette-override)
      (last-four-command-arguments)
    (multiple-value-bind (games base-palette base-settings-icon)
        (validate-catalog (read-catalog source))
      (let ((palette base-palette)
            (settings-icon base-settings-icon))
        (when (probe-file palette-override)
          (multiple-value-bind (override-palette override-settings-icon)
              (validate-appearance-override (read-catalog palette-override))
            (setf palette override-palette)
            (when override-settings-icon
              (setf settings-icon override-settings-icon))))
        (emit-outputs-atomically games palette settings-icon
                                 games-output palette-output)
        (format t
                "catalog: wrote ~D games, ~D palette roles, and icon ~A~%"
                (length games) (length palette) settings-icon)))))

(defun quit-process (status)
  #+ecl (ext:quit status)
  #+sbcl (sb-ext:exit :code status)
  #-(or ecl sbcl) (error "unsupported Common Lisp implementation"))

(handler-case
    (progn
      (main)
      (quit-process 0))
  (error (condition)
    (format *error-output* "catalog: ~A~%" condition)
    (finish-output *error-output*)
    (quit-process 1)))

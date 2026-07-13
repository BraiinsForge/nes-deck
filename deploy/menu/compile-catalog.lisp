;;;; Validate games.sexp and atomically emit the Deck menu's TSV manifest.
;;;; The catalog data is deliberately limited to printable ASCII so the same
;;;; output is produced regardless of the minimal ECL runtime's locale.

(defpackage #:nes-deck-catalog
  (:use #:common-lisp))

(in-package #:nes-deck-catalog)

(defconstant +schema-version+ 3)
(defconstant +maximum-games+ 18)
(defconstant +maximum-catalog-bytes+ 65536)
(defconstant +rom-root+ "/mnt/data/nes-deck/")
(defparameter +catalog-keys+ '(:version :games))
(defparameter +game-keys+
  '(:id :title :system :rom :color))

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

(defun decode-plist (value allowed-keys context)
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
    (dolist (key allowed-keys)
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
               (member value '(:nes :gb :gbc :chip8 :deck) :test #'eq))
    (catalog-error
     "game :system must be one of :nes, :gb, :gbc, :chip8, or :deck"))
  (string-downcase (symbol-name value)))

(defun validate-rom-path (value system)
  (validate-text-field value "game :rom" 512)
  (let ((expected-suffix
          (cond ((eq system :nes) ".nes")
                ((eq system :gb) ".gb")
                ((eq system :gbc) ".gbc")
                ((eq system :chip8) ".ch8")
                ((eq system :deck) ".sexp")
                (t (catalog-error "unsupported game system ~S" system)))))
    (unless (and (string-prefix-p +rom-root+ value)
               (string-suffix-p expected-suffix value)
               (every #'rom-path-character-p value)
               (not (search "//" value))
               (not (search "/./" value))
               (not (search "/../" value)))
      (catalog-error
       "game :rom must be a normalized ~A path below ~A"
       expected-suffix +rom-root+)))
  value)

(defun hexadecimal-character-p (character)
  (or (digit-char-p character)
      (find character "ABCDEFabcdef" :test #'char=)))

(defun validate-color (value)
  (unless (and (stringp value)
               (= (length value) 7)
               (char= (char value 0) #\#)
               (every #'hexadecimal-character-p (subseq value 1)))
    (catalog-error "game :color must have the form #RRGGBB"))
  (string-upcase value))

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
    games))

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

(defun emit-tsv-atomically (games output)
  ;; TEMP is in OUTPUT's directory, making the final POSIX rename atomic.
  (let* ((temporary (format nil "~A.tmp.~D" output (ext:getpid)))
         (committed nil))
    (unwind-protect
         (progn
           (with-open-file (stream temporary
                                   :direction :output
                                   :if-exists :supersede
                                   :if-does-not-exist :create)
             (write-tsv games stream)
             (finish-output stream))
           ;; ECL's :IF-EXISTS :SUPERSEDE maps to one POSIX rename(2), unlike
           ;; deleting OUTPUT first (which would create a missing-file window).
           (rename-file temporary output :if-exists :supersede)
           (setf committed t))
      (unless committed
        (when (probe-file temporary)
          (ignore-errors (delete-file temporary)))))))

(defun last-two-command-arguments ()
  ;; ECL includes its own options in EXT:COMMAND-ARGS.  The two fixed launcher
  ;; arguments are intentionally taken from the tail so this works both with
  ;; --shell and with ECL builds that omit processed options from the result.
  (let* ((arguments (ext:command-args))
         (count (length arguments)))
    (when (< count 2)
      (catalog-error "usage: ecl --norc --shell compile-catalog.lisp SOURCE OUTPUT"))
    (values (nth (- count 2) arguments)
            (nth (1- count) arguments))))

(defun main ()
  (multiple-value-bind (source output) (last-two-command-arguments)
    (let ((games (validate-catalog (read-catalog source))))
      (emit-tsv-atomically games output)
      (format t "catalog: wrote ~D games to ~A~%" (length games) output))))

(handler-case
    (progn
      (main)
      (ext:quit 0))
  (error (condition)
    (format *error-output* "catalog: ~A~%" condition)
    (finish-output *error-output*)
    (ext:quit 1)))

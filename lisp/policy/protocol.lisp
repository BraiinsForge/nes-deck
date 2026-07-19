(in-package #:retro-deck)


;;;; -- Bounded S-expression protocol --

(defconstant +policy-protocol-version+ 1)
(defconstant +policy-maximum-line-bytes+ (* 64 1024))
(defconstant +policy-maximum-depth+ 16)
(defconstant +policy-maximum-values+ 1024)
(defconstant +policy-maximum-error-length+ 512)


(defun policy--protocol-error (control &rest arguments)
  "Signal a structured protocol error formatted from CONTROL and ARGUMENTS."
  (error 'policy-protocol-error
         :reason (apply #'format nil control arguments)))


(defun policy--disallowed-reader-syntax (stream character)
  "Reject reader syntax outside the policy data vocabulary."
  (declare (ignore stream))
  (policy--protocol-error "reader syntax ~C is not permitted" character))


(defun policy--readtable ()
  "Construct the restricted readtable used for policy messages."
  (let ((readtable (copy-readtable nil)))
    (dolist (character '(#\# #\' #\` #\, #\;))
      (set-macro-character character
                           #'policy--disallowed-reader-syntax
                           nil
                           readtable))
    readtable))


(defun policy--whitespace-character-p (character)
  "Return true when CHARACTER is protocol whitespace."
  (or (char= character #\Space)
      (char= character #\Tab)
      (char= character #\Return)
      (char= character #\Newline)))


(defun policy--control-character-p (character)
  "Return true when CHARACTER cannot occur inside a wire string."
  (let ((code (char-code character)))
    (or (< code 32)
        (= code 127))))


(defun policy--utf-8-character-length (character)
  "Return the UTF-8 byte length of CHARACTER, or reject an invalid scalar."
  (let ((code (char-code character)))
    (cond
      ((<= code #x7f) 1)
      ((<= code #x7ff) 2)
      ((<= #xd800 code #xdfff)
       (policy--protocol-error "string contains a Unicode surrogate"))
      ((<= code #xffff) 3)
      ((<= code #x10ffff) 4)
      (t
       (policy--protocol-error "string contains an invalid Unicode scalar")))))


(defun policy--utf-8-length (string)
  "Return the number of bytes needed to encode STRING as UTF-8."
  (loop for character across string
        sum (policy--utf-8-character-length character)))


(defun policy--validate-data (value)
  "Validate VALUE against the bounded policy wire data model."
  (let ((values-seen 0))
    (labels ((visit (current depth)
               (incf values-seen)
               (when (> values-seen +policy-maximum-values+)
                 (policy--protocol-error "message contains too many values"))
               (when (> depth +policy-maximum-depth+)
                 (policy--protocol-error "message is nested too deeply"))
               (cond
                 ((or (null current)
                      (eq current t)
                      (keywordp current))
                  current)
                 ((integerp current)
                  (unless (<= (- (expt 2 63)) current (1- (expt 2 63)))
                    (policy--protocol-error
                     "integer is outside the signed 64-bit range"))
                  current)
                 ((stringp current)
                  (when (find-if #'policy--control-character-p current)
                    (policy--protocol-error
                     "string contains a control character"))
                  current)
                 ((consp current)
                  (let ((cursor current)
                        (seen   (make-hash-table :test #'eq)))
                    (loop
                      (when (gethash cursor seen)
                        (policy--protocol-error "circular lists are not permitted"))
                      (setf (gethash cursor seen) t)
                      (visit (first cursor) (1+ depth))
                      (let ((next (rest cursor)))
                        (cond
                          ((null next)
                           (return current))
                          ((consp next)
                           (setf cursor next))
                          (t
                           (policy--protocol-error
                            "dotted lists are not permitted")))))))
                 (t
                  (policy--protocol-error
                   "value of type ~S is not permitted"
                   (type-of current))))))
      (visit value 0))))


(defun policy--read-form (line)
  "Read and validate exactly one policy form from bounded string LINE."
  (unless (stringp line)
    (policy--protocol-error "message must be a string"))
  (when (> (policy--utf-8-length line) +policy-maximum-line-bytes+)
    (policy--protocol-error "message exceeds the byte limit"))
  (handler-case
      (let ((*read-eval* nil)
            (*readtable* (policy--readtable))
            (*package* (find-package '#:retro-deck)))
        (multiple-value-bind (form position)
            (read-from-string line t nil)
          (unless (every #'policy--whitespace-character-p
                         (subseq line position))
            (policy--protocol-error "message has trailing input"))
          (policy--validate-data form)))
    (policy-error (condition)
      (error condition))
    (condition (condition)
      (policy--protocol-error "reader rejected input: ~A" condition))))


(defun policy--write-form (form)
  "Validate and serialize one policy FORM without pretty-print line breaks."
  (policy--validate-data form)
  (let ((*print-array* nil)
        (*print-circle* nil)
        (*print-case* :downcase)
        (*print-level* nil)
        (*print-length* nil)
        (*print-pretty* nil)
        (*print-readably* t))
    (let ((encoded (write-to-string form)))
      (when (> (policy--utf-8-length encoded) +policy-maximum-line-bytes+)
        (policy--protocol-error "response exceeds the byte limit"))
      encoded)))


(defun policy--plist-value (plist key)
  "Return the unique value for KEY in proper even PLIST."
  (unless (evenp (length plist))
    (policy--protocol-error "property list has an odd number of values"))
  (let ((found-p nil)
        (value   nil))
    (loop for (candidate-key candidate-value) on plist by #'cddr
          when (eq candidate-key key)
            do (when found-p
                 (policy--protocol-error "property ~S is repeated" key))
               (setf found-p t
                     value candidate-value))
    (unless found-p
      (policy--protocol-error "property ~S is missing" key))
    value))


(defun policy--validate-request-keys (plist)
  "Reject unknown keys in request PLIST."
  (loop for cursor on plist by #'cddr
        for key = (first cursor)
        unless (member key '(:version :id :hook :arguments) :test #'eq)
          do (policy--protocol-error "request property ~S is unknown" key)))


(defun policy--request-id-or-zero (form)
  "Recover a valid request identifier from FORM for an error response."
  (handler-case
      (if (and (consp form) (eq (first form) :request))
          (let ((identifier (policy--plist-value (rest form) :id)))
            (if (and (integerp identifier)
                     (<= 0 identifier (1- (expt 2 63))))
                identifier
                0))
          0)
    (condition ()
      0)))


(defun policy--handle-request (form)
  "Validate request FORM, call its hook, and return a response form."
  (unless (and (consp form) (eq (first form) :request))
    (policy--protocol-error "message is not a request"))
  (let ((properties (rest form)))
    (policy--validate-request-keys properties)
    (let ((version   (policy--plist-value properties :version))
          (identifier (policy--plist-value properties :id))
          (hook      (policy--plist-value properties :hook))
          (arguments (policy--plist-value properties :arguments)))
      (unless (and (integerp version)
                   (= version +policy-protocol-version+))
        (policy--protocol-error "protocol version ~S is unsupported" version))
      (unless (and (integerp identifier)
                   (<= 0 identifier (1- (expt 2 63))))
        (policy--protocol-error "request identifier is invalid"))
      (unless (keywordp hook)
        (policy--protocol-error "hook name must be a keyword"))
      (policy--validate-data arguments)
      (let ((value (call-policy-hook hook arguments)))
        (policy--validate-data value)
        (list :response
              :version +policy-protocol-version+
              :id identifier
              :status :ok
              :value value)))))


(defun policy--clean-error-message (condition)
  "Return a bounded, single-line description of CONDITION."
  (let* ((raw   (princ-to-string condition))
         (clean (map 'string
                     (lambda (character)
                       (if (policy--control-character-p character)
                           #\Space
                           character))
                     raw)))
    (if (> (length clean) +policy-maximum-error-length+)
        (subseq clean 0 +policy-maximum-error-length+)
        clean)))


(defun policy--error-response (identifier condition)
  "Construct an error response for IDENTIFIER and CONDITION."
  (list :response
        :version +policy-protocol-version+
        :id identifier
        :status :error
        :message (policy--clean-error-message condition)))


(defun policy--process-line (line)
  "Process one request LINE and return one encoded response line."
  (let ((form nil))
    (handler-case
        (progn
          (setf form (policy--read-form line))
          (policy--write-form (policy--handle-request form)))
      (condition (condition)
        (policy--write-form
         (policy--error-response (policy--request-id-or-zero form)
                                 condition))))))

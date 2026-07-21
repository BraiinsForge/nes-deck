(in-package #:retro-deck)


;;;; -- Supervised policy worker --

(defun policy--environment-variable (name)
  "Return environment variable NAME on supported Lisp implementations."
  #+sbcl (sb-ext:posix-getenv name)
  #+ecl (ext:getenv name)
  #-(or sbcl ecl)
  (error 'policy-error))


(defun policy--lisp-source-path-p (path)
  "Return true when PATH names a file with a `.lisp` extension."
  (let ((type (pathname-type path)))
    (and type (string-equal type "lisp"))))


(defun policy--directory-pathname (directory)
  "Return DIRECTORY as a pathname ending in a directory separator."
  (let* ((path (pathname directory))
         (name (namestring path)))
    (pathname
     (if (and (plusp (length name))
              (char= (char name (1- (length name))) #\/))
         name
         (concatenate 'string name "/")))))


(defun policy--site-files (directory)
  "Return lexically ordered Lisp files in existing DIRECTORY."
  (cond
    ((or (null directory)
         (and (stringp directory) (string= directory "")))
     nil)
    (t
     (let* ((base (policy--directory-pathname directory))
            (wildcard (merge-pathnames (make-pathname :name :wild
                                                      :type "lisp")
                                       base))
            (files (handler-case (directory wildcard)
                     (file-error () nil))))
       (sort (remove-if-not #'policy--lisp-source-path-p files)
             #'string<
             :key #'namestring)))))


(defun policy--load-site-directory (directory)
  "Load trusted Lisp overrides from DIRECTORY in lexical order."
  (dolist (path (policy--site-files directory))
    (load path :verbose nil :print nil))
  t)


(defun policy--read-bounded-line (stream)
  "Read one bounded protocol line from STREAM, or return `:eof`."
  (let ((characters (make-array 128
                                :element-type 'character
                                :adjustable t
                                :fill-pointer 0))
        (bytes-seen 0))
    (loop
      for character = (read-char stream nil :eof)
      do (cond
           ((eq character :eof)
            (return (if (zerop (length characters))
                        :eof
                        (coerce characters 'string))))
           ((char= character #\Newline)
            (return (coerce characters 'string)))
           (t
            (incf bytes-seen (policy--utf-8-character-length character))
            (when (> bytes-seen +policy-maximum-line-bytes+)
              (policy--protocol-error "message exceeds the byte limit"))
            (vector-push-extend character characters))))))


(defun run-policy-worker (&key
                            (input *standard-input*)
                            (output *standard-output*)
                            (site-directory
                             (policy--environment-variable
                              "RETRO_DECK_LISP_SITE_DIR")))
  "Load policy, announce readiness, and serve bounded requests over streams."
  (install-default-policy-hooks)
  (policy--load-site-directory site-directory)
  (write-line
   (policy--write-form
    (list :ready :version +policy-protocol-version+))
   output)
  (finish-output output)
  (loop
    for line = (policy--read-bounded-line input)
    until (eq line :eof)
    do (write-line (policy--process-line line) output)
       (finish-output output))
  t)

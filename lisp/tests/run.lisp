(require "asdf")

(let* ((test-directory   (uiop:pathname-directory-pathname *load-truename*))
       (lisp-directory   (uiop:pathname-parent-directory-pathname
                          test-directory))
       (system-path      (merge-pathnames "retro-deck.asd" lisp-directory)))
  (asdf:load-asd system-path)
  (asdf:load-system '#:retro-deck))

(in-package #:retro-deck)


;;;; -- Minimal test harness --

(defvar *test-failures* 0)


(defun test-check (description condition)
  "Record whether CONDITION satisfies test DESCRIPTION."
  (unless condition
    (incf *test-failures*)
    (format *error-output* "FAIL: ~A~%" description)))


(defun test-check-equal (description expected actual)
  "Record whether EXPECTED and ACTUAL are structurally equal."
  (test-check description (equal expected actual)))


(defun test-condition-p (condition-type function)
  "Return true when FUNCTION signals CONDITION-TYPE."
  (handler-case
      (progn
        (funcall function)
        nil)
    (condition (condition)
      (typep condition condition-type))))


;;;; -- Hook behavior --

(install-default-policy-hooks)

(test-check-equal
 "default dashboard startup is ordered policy data"
 '(:applications
   ((:lua "LUA REPL" "#5F87FF")
    (:lisp "LISP REPL" "#AFD75F")
    (:python "PYTHON REPL" "#FFD700")
    (:scheme "SCHEME REPL" "#87D787")
    (:chiptunes "CHIPTUNES" "#FF8700")
    (:terminal "TERMINAL" "#5F87AF")
    (:reboot "REBOOT" "#D75F5F"))
   :gamepad
   ((:button 288 :back)
    (:button 289 :back)
    (:button 290 :confirm)
    (:button 291 :confirm)
    (:button 294 :back)
    (:button 295 :confirm)
    (:axis 0 :left :right)
    (:axis 1 :up :down)))
 (call-policy-hook :dashboard/startup nil))

(test-check
 "dashboard startup policy rejects arguments"
 (test-condition-p
  'policy-hook-error
  (lambda ()
    (call-policy-hook :dashboard/startup '(:unexpected t)))))

(test-check-equal
 "default timer policy preserves an exact result"
 '(:display-centiseconds 1000 :cue :exact)
 (call-policy-hook :ten-seconds/result
                   '(:elapsed-centiseconds 1000 :input :touch)))

(test-check-equal
 "default timer policy preserves a miss"
 '(:display-centiseconds 987 :cue :miss)
 (call-policy-hook :ten-seconds/result
                   '(:elapsed-centiseconds 987 :input :controller-a)))

(test-check
 "timer policy rejects an unknown input source"
 (test-condition-p
  'policy-hook-error
  (lambda ()
    (call-policy-hook :ten-seconds/result
                      '(:elapsed-centiseconds 987 :input :keyboard)))))

(register-policy-hook :ten-seconds/result
                      (lambda (arguments)
                        (declare (ignore arguments))
                        '(:display-centiseconds 1 :cue :miss)))
(test-check-equal
 "a later trusted registration replaces base behavior"
 '(:display-centiseconds 1 :cue :miss)
 (call-policy-hook :ten-seconds/result
                   '(:elapsed-centiseconds 987 :input :touch)))


;;;; -- Wire behavior --

(install-default-policy-hooks)

(test-check-equal
 "worker processes a valid request"
 "(:response :version 1 :id 7 :status :ok :value (:display-centiseconds 1000 :cue :exact))"
 (policy--process-line
  "(:request :version 1 :id 7 :hook :ten-seconds/result :arguments (:elapsed-centiseconds 1000 :input :touch))"))

(test-check
 "reader evaluation is rejected"
 (search ":status :error"
         (policy--process-line
          "(:request :version 1 :id 8 :hook :ten-seconds/result :arguments #.(quit))")))

(test-check
 "plain symbols are rejected"
 (search ":status :error"
         (policy--process-line
          "(:request :version 1 :id 9 :hook secret :arguments nil)")))

(test-check
 "a non-integer protocol version is rejected cleanly"
 (search ":status :error"
         (policy--process-line
          "(:request :version :one :id 9 :hook :ten-seconds/result :arguments nil)")))

(test-check
 "multiple input forms are rejected"
 (search ":status :error"
         (policy--process-line "nil nil")))

(test-check
 "unknown hooks produce a bounded error response"
 (search ":status :error"
         (policy--process-line
          "(:request :version 1 :id 10 :hook :missing :arguments nil)")))

(test-check-equal
 "UTF-8 bounds count encoded bytes instead of characters"
 8
 (policy--utf-8-length
  (coerce (list #\a (code-char #x20ac) (code-char #x1f642)) 'string)))

(test-check
 "wire validation rejects C1 controls inside strings"
 (test-condition-p
  'policy-protocol-error
  (lambda ()
    (policy--validate-data (string (code-char #x85))))))

(test-check
 "wire validation accepts only canonical Rust keywords"
 (and (policy--valid-keyword-p :ten-seconds/result)
      (test-condition-p
       'policy-protocol-error
       (lambda ()
         (policy--validate-data (intern "lower" '#:keyword))))
      (test-condition-p
       'policy-protocol-error
       (lambda ()
         (policy--validate-data (intern "BAD SPACE" '#:keyword))))))

(labels ((nested-list (levels)
           (if (zerop levels)
               0
               (list (nested-list (1- levels))))))
  (test-check
   "wire validation accepts the shared nesting limit"
   (not (test-condition-p
         'policy-protocol-error
         (lambda ()
           (policy--validate-data
            (nested-list +policy-maximum-depth+))))))
  (test-check
   "wire validation rejects one list beyond the shared nesting limit"
   (test-condition-p
    'policy-protocol-error
    (lambda ()
      (policy--validate-data
       (nested-list (1+ +policy-maximum-depth+)))))))


;;;; -- Worker framing --

(let ((input  (make-string-input-stream
               (format nil
                       "(:request :version 1 :id 11 :hook :ten-seconds/result :arguments (:elapsed-centiseconds 1000 :input :touch))~%")))
      (output (make-string-output-stream)))
  (run-policy-worker :input input :output output :site-directory nil)
  (test-check-equal
   "worker announces readiness before serving requests"
   (format nil
           "(:ready :version 1)~%(:response :version 1 :id 11 :status :ok :value (:display-centiseconds 1000 :cue :exact))~%")
   (get-output-stream-string output)))


;;;; -- Ordered local overrides --

(let* ((temporary-root (merge-pathnames
                        (format nil "retro-deck-lisp-test-~D-~D/"
                                (get-universal-time)
                                (get-internal-real-time))
                        (uiop:temporary-directory)))
       (first-path     (merge-pathnames "10-first.lisp" temporary-root))
       (second-path    (merge-pathnames "20-second.lisp" temporary-root)))
  (unwind-protect
       (progn
         (ensure-directories-exist first-path)
         (with-open-file (stream first-path
                                 :direction :output
                                 :if-exists :error
                                 :if-does-not-exist :create)
           (write-line "(in-package #:retro-deck)" stream)
           (write-line
            "(register-policy-hook :test/order (lambda (arguments) (declare (ignore arguments)) 1))"
            stream))
         (with-open-file (stream second-path
                                 :direction :output
                                 :if-exists :error
                                 :if-does-not-exist :create)
           (write-line "(in-package #:retro-deck)" stream)
           (write-line
            "(register-policy-hook :test/order (lambda (arguments) (declare (ignore arguments)) 2))"
            stream))
         (policy--load-site-directory temporary-root)
         (test-check-equal
          "site overrides load in lexical order"
          2
          (call-policy-hook :test/order nil)))
    (when (probe-file temporary-root)
      (uiop:delete-directory-tree temporary-root :validate t))))


;;;; -- Failed local startup --

(let* ((temporary-root (merge-pathnames
                        (format nil "retro-deck-lisp-broken-test-~D-~D/"
                                (get-universal-time)
                                (get-internal-real-time))
                        (uiop:temporary-directory)))
       (broken-path    (merge-pathnames "10-broken.lisp" temporary-root))
       (output         (make-string-output-stream))
       (failed-p       nil))
  (unwind-protect
       (progn
         (ensure-directories-exist broken-path)
         (with-open-file (stream broken-path
                                 :direction :output
                                 :if-exists :error
                                 :if-does-not-exist :create)
           (write-line "(error \"intentional startup failure\")" stream))
         (let ((*error-output* (make-broadcast-stream)))
           (handler-case
               (run-policy-worker
                :input (make-string-input-stream "")
                :output output
                :site-directory temporary-root)
             (error ()
               (setf failed-p t))))
         (test-check "a broken site override aborts worker startup" failed-p)
         (test-check-equal
          "a broken site override cannot announce readiness"
          ""
          (get-output-stream-string output)))
    (when (probe-file temporary-root)
      (uiop:delete-directory-tree temporary-root :validate t))))


;;;; -- Result --

(if (zerop *test-failures*)
    (progn
      (format t "retro-deck-lisp-tests: OK~%")
      (uiop:quit 0))
    (progn
      (format *error-output*
              "retro-deck-lisp-tests: ~D failure(s)~%"
              *test-failures*)
      (uiop:quit 1)))

(in-package #:retro-deck)


;;;; -- 10 Seconds policy --

(defun ten-seconds--validate-arguments (arguments)
  "Return validated elapsed centiseconds and input from ARGUMENTS."
  (unless (listp arguments)
    (error 'policy-hook-error
           :hook-name :ten-seconds/result
           :reason "arguments must be a property list"))
  (handler-case
      (progn
        (loop for cursor on arguments by #'cddr
              for key = (first cursor)
              unless (member key '(:elapsed-centiseconds :input) :test #'eq)
                do (policy--protocol-error
                    "10 Seconds argument ~S is unknown"
                    key))
        (let ((elapsed (policy--plist-value arguments :elapsed-centiseconds))
              (input   (policy--plist-value arguments :input)))
          (unless (and (integerp elapsed) (<= 0 elapsed 9999))
            (policy--protocol-error
             "elapsed centiseconds must be between 0 and 9999"))
          (unless (member input '(:touch :controller-a) :test #'eq)
            (policy--protocol-error "input source ~S is unsupported" input))
          (values elapsed input)))
    (policy-protocol-error (condition)
      (error 'policy-hook-error
             :hook-name :ten-seconds/result
             :reason (policy-protocol-error-reason condition)))))


(defun ten-seconds--default-result (arguments)
  "Return the honest default timer result for validated ARGUMENTS."
  (multiple-value-bind (elapsed input)
      (ten-seconds--validate-arguments arguments)
    (declare (ignore input))
    (list :display-centiseconds elapsed
          :cue (if (= elapsed 1000) :exact :miss))))

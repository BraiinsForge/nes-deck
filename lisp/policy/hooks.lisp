(in-package #:retro-deck)


;;;; -- Policy hook registry --

(defvar *policy-hooks* (make-hash-table :test #'eq)
  "Registered trusted behavior hooks keyed by keyword name.")


(defun register-policy-hook (name function)
  "Install FUNCTION as the implementation of keyword hook NAME."
  (unless (keywordp name)
    (error 'policy-hook-error
           :hook-name :invalid-hook-name
           :reason "hook names must be keywords"))
  (unless (functionp function)
    (error 'policy-hook-error
           :hook-name name
           :reason "hook implementation must be a function"))
  (setf (gethash name *policy-hooks*) function)
  name)


(defun call-policy-hook (name arguments)
  "Invoke registered hook NAME with validated data ARGUMENTS."
  (unless (keywordp name)
    (error 'policy-hook-error
           :hook-name :invalid-hook-name
           :reason "hook names must be keywords"))
  (multiple-value-bind (function present-p)
      (gethash name *policy-hooks*)
    (unless present-p
      (error 'policy-hook-error
             :hook-name name
             :reason "hook is not registered"))
    (funcall function arguments)))


(defun policy--clear-hooks ()
  "Remove every registered hook before installing one coherent policy set."
  (clrhash *policy-hooks*))

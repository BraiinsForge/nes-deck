(in-package #:retro-deck)


;;;; -- Tracked policy installation --

(defun install-default-policy-hooks ()
  "Install the complete tracked behavior set before local overrides load."
  (policy--clear-hooks)
  (register-policy-hook :dashboard/applications
                        #'dashboard--default-applications)
  (register-policy-hook :ten-seconds/result
                        #'ten-seconds--default-result)
  t)

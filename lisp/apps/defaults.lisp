(in-package #:retro-deck)


;;;; -- Tracked policy installation --

(defun install-default-policy-hooks ()
  "Install the complete tracked behavior set before local overrides load."
  (policy--clear-hooks)
  (register-policy-hook :dashboard/startup
                        #'dashboard--default-startup)
  (register-policy-hook :ten-seconds/result
                        #'ten-seconds--default-result)
  t)

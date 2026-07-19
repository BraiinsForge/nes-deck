(in-package #:retro-deck)


;;;; -- Policy conditions --

(define-condition policy-error (error)
  ()
  (:documentation "Base condition for the trusted policy runtime."))


(define-condition policy-protocol-error (policy-error)
  ((reason :initarg :reason
           :reader policy-protocol-error-reason
           :type string))
  (:report
   (lambda (condition stream)
     (format stream "Invalid policy message: ~A"
             (policy-protocol-error-reason condition))))
  (:documentation "A policy message is malformed or outside its bounds."))


(define-condition policy-hook-error (policy-error)
  ((hook-name :initarg :hook-name
              :reader policy-hook-error-hook-name
              :type keyword)
   (reason    :initarg :reason
              :reader policy-hook-error-reason
              :type string))
  (:report
   (lambda (condition stream)
     (format stream "Policy hook ~S failed: ~A"
             (policy-hook-error-hook-name condition)
             (policy-hook-error-reason condition))))
  (:documentation "A requested policy hook is absent or rejected its data."))

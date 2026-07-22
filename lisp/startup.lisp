(defpackage #:retrodeck.native
  (:use)
  (:export #:abi-version))

(defpackage #:retrodeck
  (:use #:cl)
  (:import-from #:retrodeck.native #:abi-version)
  (:export #:main))

(in-package #:retrodeck)

(defconstant +native-abi-version+ 1)

(defun main ()
  (unless (= (abi-version) +native-abi-version+)
    (error "Native ABI mismatch"))
  (format t "retrodeck: Common Lisp startup loaded~%")
  (finish-output)
  0)

(let ((local (merge-pathnames "local.lisp" *load-truename*)))
  (when (probe-file local)
    (load local :verbose nil :print nil)))

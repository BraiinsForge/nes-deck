(let ((*standard-output* *error-output*)
      (*trace-output*    *error-output*)
      (*load-verbose*    nil)
      (*compile-verbose* nil)
      (script-directory (make-pathname :name nil
                                       :type nil
                                       :defaults *load-truename*)))
  ;; Loading ASDF on the small ARM target costs far more memory and time than
  ;; loading this fixed source set directly.
  (dolist (relative-path '("package.lisp"
                           "policy/conditions.lisp"
                           "policy/hooks.lisp"
                           "policy/protocol.lisp"
                           "apps/dashboard.lisp"
                           "apps/ten-seconds.lisp"
                           "apps/defaults.lisp"
                           "policy/worker.lisp"))
    (load (merge-pathnames relative-path script-directory)
          :verbose nil
          :print nil)))

(funcall (find-symbol "RUN-POLICY-WORKER" '#:retro-deck))

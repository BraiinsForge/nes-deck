(let ((*standard-output* *error-output*)
      (*trace-output*    *error-output*))
  (require "asdf"))

(let ((*standard-output* *error-output*)
      (*trace-output*    *error-output*)
      (*load-verbose*    nil)
      (*compile-verbose* nil))
  (let* ((script-directory (uiop:pathname-directory-pathname *load-truename*))
         (system-path      (merge-pathnames "retro-deck.asd"
                                           script-directory)))
    (asdf:load-asd system-path)
    (asdf:load-system '#:retro-deck)))

(funcall (find-symbol "RUN-POLICY-WORKER" '#:retro-deck))

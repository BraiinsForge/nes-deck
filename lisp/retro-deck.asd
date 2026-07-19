(asdf:defsystem #:retro-deck
  :description "Trusted Common Lisp behavior layer for Retro Deck"
  :version "0.1.0"
  :license "GPL-3.0-only"
  :depends-on (#:uiop)
  :serial t
  :components ((:file "package")
               (:module "policy"
                :serial t
                :components ((:file "conditions")
                             (:file "hooks")
                             (:file "protocol")))
               (:module "apps"
                :serial t
                :components ((:file "ten-seconds")))
               (:file "policy-worker"
                :pathname "policy/worker")))

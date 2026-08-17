(define build (lambda (n) (if (= n 0) (list) (cons n (build (- n 1))))))
(build 3)

(define build2 (lambda (f n) (if (= n 0) (list) (cons n (build2 f (- n 1))))))
(define square (lambda (x) (* x x)))
(build2 square 3)

(define build3 (lambda (f n) (if (= n 0) (list) (cons (f n) (build3 f (- n 1))))))
(define square (lambda (x) (* x x)))
(build3 square 3)

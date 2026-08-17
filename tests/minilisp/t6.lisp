(define square (lambda (x) (* x x)))
(define rep (lambda (f n) (if (= n 0) 0 (+ (f n) (rep f (- n 1))))))
(rep square 3)

(define apply1 (lambda (f x) (f x)))
(define square (lambda (x) (* x x)))
(apply1 square 7)

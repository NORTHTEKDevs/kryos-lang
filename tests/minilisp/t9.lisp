(define suml (lambda (lst) (if (null? lst) 0 (+ (car lst) (suml (cdr lst))))))
(suml (list 1 2 3 4))

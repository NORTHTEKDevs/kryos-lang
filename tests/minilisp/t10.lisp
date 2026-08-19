(define make-counter (lambda () (let ((n 0)) (lambda () (set! n (+ n 1)) n))))
(define c1 (make-counter))
(c1)
(c1)

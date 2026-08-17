(define count (lambda (n) (if (= n 0) 0 (+ 1 (count (- n 1))))))
(count 5)

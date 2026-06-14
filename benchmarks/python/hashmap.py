def main():
    n = 1000000
    m = {}
    for i in range(n):
        m[i] = i * 2
    s = 0
    for j in range(n):
        s += m[j]
    print(s)
main()

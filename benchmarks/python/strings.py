def main():
    n = 100000
    token = "ab"
    s = ""
    for _ in range(n):
        s += token
    slen = len(s)
    checksum = sum(ord(c) for c in s)
    print(slen)
    print(checksum)

main()

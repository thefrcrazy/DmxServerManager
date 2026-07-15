export function passwordMeetsPolicy(password: string): boolean {
    const encodedLength = new TextEncoder().encode(password).byteLength;
    return encodedLength >= 12
        && encodedLength <= 256
        && /\p{Uppercase}/u.test(password)
        && /\p{Lowercase}/u.test(password)
        && /[0-9]/.test(password)
        && /[^\p{Letter}\p{Number}]/u.test(password);
}

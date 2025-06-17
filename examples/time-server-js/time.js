async function getCurrentTime() {
    return new Date().toISOString();
}

export const time = {
    getCurrentTime
};
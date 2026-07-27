export function createSearchRequestOwnership() {
  let latestRequest = 0;

  return {
    begin() {
      latestRequest += 1;
      return latestRequest;
    },
    owns(request: number) {
      return request === latestRequest;
    },
    cancel(request: number) {
      if (request === latestRequest) latestRequest += 1;
    }
  };
}

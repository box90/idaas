import { QueryClient } from "@tanstack/react-query"
import { clearKey } from "./auth"

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: 1, staleTime: 30_000 },
  },
})

export function handleUnauthorized() {
  clearKey()
  queryClient.clear()
  window.location.replace("/login")
}

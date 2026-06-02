const KEY = "idass_mgmt_key"

export const getKey = (): string | null => sessionStorage.getItem(KEY)
export const setKey = (key: string): void => sessionStorage.setItem(KEY, key)
export const clearKey = (): void => sessionStorage.removeItem(KEY)
export const getKeyTail = (): string => {
  const k = getKey()
  return k ? `●●●●●●●●${k.slice(-8)}` : ""
}

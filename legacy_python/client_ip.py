"""Only accept forwarding headers from explicitly configured proxies."""
import ipaddress, os

def address(handler):
    direct=ipaddress.ip_address(handler.client_address[0])
    if getattr(direct,'ipv4_mapped',None): direct=direct.ipv4_mapped
    trusted=[ipaddress.ip_network(v.strip()) for v in os.environ.get('TRUSTED_PROXIES','').split(',') if v.strip()]
    def is_trusted(ip):return any(ip in network for network in trusted)
    if not is_trusted(direct):return direct
    try:
        chain=[ipaddress.ip_address(v.strip()) for v in handler.headers.get('X-Forwarded-For','').split(',') if v.strip()]
        current=direct
        for candidate in reversed(chain):
            if not is_trusted(current):break
            current=candidate.ipv4_mapped if getattr(candidate,'ipv4_mapped',None) else candidate
        return current
    except ValueError:return direct
